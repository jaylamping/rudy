//! Home-ramp closed loop that walks a motor from a current position to a
//! principal-angle target via the shortest signed path.
//!
//! Extracted from `crate::api::home::run_homer` so the same loop body is
//! callable from:
//!
//! - the operator-initiated `POST /api/motors/:role/home` HTTP handler
//!   (`crate::api::home`), which gates on `BootState::InBand`/`Homed`,
//!   audit-logs, and emits `SafetyEvent::Homed`;
//! - the boot orchestrator (`crate::boot_orchestrator`, lands in Phase
//!   C.5), which detects an InBand commissioned motor on first valid
//!   telemetry and drives it to the per-motor `predefined_home_rad`
//!   without operator intervention.
//!
//! Each tick:
//!   1. Reads the latest type-2 telemetry row from `state.latest` (when
//!      `real_can` is present) and applies `tracking_freshness_max_age_ms`.
//!      Stale/missing rows **hold** the ramp setpoint for that tick so the
//!      setpoint cannot outrun a frozen `mech_pos_rad`.
//!   2. Advances the setpoint by at most `step_size_rad` toward the target
//!      only when telemetry is fresh (always advances in mock mode).
//!   3. Re-runs the path-aware band check on the current measured position
//!      vs. the next setpoint.
//!   4. Issues a velocity setpoint sized so the motor advances by
//!      ~`step_size_rad` per `tick_interval_ms` (default ~0.4 rad/s ≈
//!      23 deg/s), in the direction of the remaining signed delta.
//!   5. Aborts on tracking error after `tracking_error_debounce_ticks`
//!      consecutive **fresh** over-budget samples (post grace), on
//!      band/path violation after `band_violation_debounce_ticks`
//!      consecutive fresh out-of-band samples (post grace) — the
//!      debounce keeps a single-tick gravity-driven overshoot of a
//!      home target near the band edge from killing the whole run —
//!      or on `homer_timeout_ms`.
//!
//! Velocity profile: the per-tick velocity command is tapered by the
//! SMALLER of (a) distance from `last_measured` to the home target,
//! (b) distance from `last_measured` to the nearest `travel_limits`
//! edge in the direction of motion, and (c) `step_size_rad` minus the
//! current overrun of `last_measured` past the virtual setpoint in the
//! direction of motion. (a) preserves the soft final approach to the
//! home target; (b) is a predictive cap that pre-decelerates the motor
//! before it can run into the band edge, independent of where the home
//! target sits in the band; (c) is a symmetric companion to (a) that
//! brakes the commanded velocity to zero whenever the motor races
//! ahead of the trajectory under gravity assist (shoulder_pitch
//! falling toward its low-gravity neutral pose with a payload), so the
//! firmware velocity loop pulls the motor back into trajectory rather
//! than letting position lead grow unboundedly. Together with the
//! band-violation debounce and the one-sided tracking-error gate (only
//! lag, not overrun, counts toward the abort), these keep
//! gravity-loaded joints from false-aborting on the very approach
//! they're supposed to be making.
//!
//! On EVERY exit path — success, abort, or timeout — the motor is
//! commanded to stop (type-4) and `state.enabled` is cleared. Mock-mode
//! (`state.real_can.is_none()`) skips the I/O and simulates instant
//! tracking so contract tests can pin the success path without
//! hardware.
//!
//! Returns `(final_pos, ticks)` on success or `(reason, last_pos)` on
//! abort. `final_pos` is the unwrapped raw mechanical position so the
//! audit log and SPA show what the multi-turn encoder actually reads.

use std::time::{Duration, Instant};

use chrono::Utc;
use tracing::{debug, info, warn};

use crate::can::angle::{PrincipalAngle, UnwrappedAngle};
use crate::can::motion::{shortest_signed_delta, wrap_to_pi};
use crate::can::travel::{enforce_position_with_path, BandCheck};
use crate::config::SafetyConfig;
use crate::inventory::Actuator;
use crate::state::SharedState;

/// Hard cap on the velocity the homer will issue. 100 deg/s expressed in
/// rad/s (~1.7453). This intentionally **exceeds** the jog endpoint's
/// `MAX_JOG_VEL_RAD_S` (0.5 rad/s ~= 28.6 deg/s) — operators commissioning a
/// new arm wanted to dial in faster homes than the jog ceiling allows. The
/// gating is the per-actuator `inventory.homing_speed_rad_s` override (UI
/// in `actuator-travel-tab.tsx`); the auto-home path on boot inherits the
/// same override, so a fast homing speed configured here will also be used
/// unattended at boot — set it to a value the joint can actually track
/// against gravity from a cold start. In practice the per-tick rate
/// (~0.4 rad/s with default `step_size_rad` and `tick_interval_ms`) is
/// well below this; the cap is a safety net for the override knob.
pub const MAX_HOMER_VEL_RAD_S: f32 = 100.0_f32.to_radians();

/// Returns `true` when the homer should abort with `tracking_error`.
///
/// The argument list is intentionally flat (gate flags, the current
/// tick's freshness/error pair, the debounce state, and the role for
/// logging) so the call site reads as a one-line decision against the
/// per-tick locals rather than another struct that has to be packed
/// every loop iteration. The unit tests below pin each gate
/// independently, which is much cleaner with positional args than with
/// a builder.
#[allow(clippy::too_many_arguments)]
fn tracking_error_should_abort(
    homer_has_real_can: bool,
    is_fresh: bool,
    ticks: u32,
    grace_ticks: u32,
    err_rad: f32,
    budget_rad: f32,
    debounce_ticks: u32,
    consec_over: &mut u32,
    role: &str,
) -> bool {
    if !homer_has_real_can || !is_fresh || ticks <= grace_ticks {
        return false;
    }
    if err_rad > budget_rad {
        *consec_over = consec_over.saturating_add(1);
        debug!(
            role = %role,
            consec_over = *consec_over,
            err_rad,
            budget_rad,
            "home_ramp: tracking error accumulating"
        );
        *consec_over >= debounce_ticks
    } else {
        *consec_over = 0;
        false
    }
}

/// Returns `true` when the homer should abort with `path_violation`.
///
/// Mirrors [`tracking_error_should_abort`] in shape — same flat
/// argument list, same gate semantics — so the loop body reads as two
/// parallel one-line decisions rather than two different control
/// shapes. The "why" is in `safety.band_violation_debounce_ticks`'s
/// docstring; the short version is that the home-ramp commands
/// velocity, not position, and a single-tick overshoot of the band
/// edge under gravity load (shoulder_pitch into a low-stop, etc.) used
/// to trip an instant abort even though the next velocity command was
/// already steering the motor back into band.
///
/// `is_violation` is `true` when this tick's
/// `enforce_position_with_path` returned `OutOfBand` or
/// `PathViolation` against `last_measured` (or, in the OutOfBand case,
/// `setpoint_unwrapped` — both feed the same gate because the
/// caller-visible reason string is the same and the recovery is the
/// same).
///
/// Mock-mode and stale-telemetry semantics match the tracking-error
/// gate: don't bite in mock mode (those paths are exercised by
/// hermetic contract tests that pin tick counts), and don't increment
/// the debounce counter on stale ticks (since `last_measured` is
/// sticky across stale stretches and the underlying physical evidence
/// hasn't actually been re-observed).
#[allow(clippy::too_many_arguments)]
fn band_violation_should_abort(
    homer_has_real_can: bool,
    is_fresh: bool,
    ticks: u32,
    grace_ticks: u32,
    is_violation: bool,
    debounce_ticks: u32,
    consec_over: &mut u32,
    role: &str,
) -> bool {
    if !homer_has_real_can || !is_fresh || ticks <= grace_ticks {
        return false;
    }
    if is_violation {
        *consec_over = consec_over.saturating_add(1);
        debug!(
            role = %role,
            consec_over = *consec_over,
            "home_ramp: band violation accumulating"
        );
        *consec_over >= debounce_ticks
    } else {
        *consec_over = 0;
        false
    }
}

/// Compute the additional velocity-magnitude cap that pre-decelerates
/// the motor before it can run into a band edge.
///
/// The home-ramp's existing taper (`approach_scale = |governing| /
/// step_size_rad`) decelerates as `last_measured` approaches the
/// **target**. That works when the home target sits comfortably inside
/// the band, but fails when the target is near (or coincident with) a
/// band edge: the motor reaches its tapered final-approach velocity
/// *and* gravity / inertia carry it past the target by ≪ step_size_rad,
/// which lands it outside the band before the reactive
/// direction-flip logic in `governing` can reverse the velocity command.
///
/// This helper closes that gap by computing the unsigned distance from
/// `last_measured` to the band edge **in the direction of motion** and
/// returning it as a cap on the per-tick travel budget. Combined with
/// the existing `approach_scale = governing / step_size_rad` taper, the
/// effective velocity command becomes
///
/// ```text
/// vel = direction * nominal_speed * (min(|governing|, dist_to_edge) /
/// step_size_rad).min(1.0)
/// ```
///
/// so the motor's commanded velocity smoothly approaches zero as it
/// nears whichever boundary it would hit first — the home target OR
/// the band edge. Importantly this is a **predictive** cap (computed
/// from the current `last_measured`, before the next velocity command
/// goes out), where the existing reactive flip via `governing` only
/// kicks in *after* the overshoot is observed on the next telemetry
/// tick.
///
/// When the inventory has no `travel_limits` for this role, returns
/// `f32::INFINITY` so the caller's `min(|governing|, _)` is a no-op
/// and behavior matches the pre-cap implementation. Also returns
/// infinity when `direction` is exactly 0 (the loop already commanded
/// `vel = 0`).
///
/// Uses principal angles (`wrap_to_pi`) on `last_measured` because the
/// stored `travel_limits` are likewise principal-angle bounds —
/// matches the convention `enforce_position_with_path` uses for the
/// abort check.
fn band_edge_distance(
    limits: Option<&crate::inventory::TravelLimits>,
    last_measured: f32,
    direction: f32,
) -> f32 {
    let Some(limits) = limits else {
        return f32::INFINITY;
    };
    if direction == 0.0 {
        return f32::INFINITY;
    }
    let cur_p = wrap_to_pi(last_measured);
    if direction > 0.0 {
        (limits.max_rad - cur_p).max(0.0)
    } else {
        (cur_p - limits.min_rad).max(0.0)
    }
}

/// Home-ramp closed loop. See module docstring for the full semantics.
///
/// `from_rad` is the operator-supplied (or telemetry-snapshotted)
/// current position; `target_rad` is the principal-angle home target.
/// Both pre-conditions — control-lock, BootState gate, band check —
/// are the caller's responsibility. This function is safe to call from
/// either an HTTP handler or the boot orchestrator; it does NOT
/// transition `BootState` itself, audit-log the outcome, or emit any
/// `SafetyEvent` — those are domain concerns the caller owns so the
/// orchestrator can route them through its own state machine.
///
/// Convenience wrapper that uses the operator-driven tracking-error
/// budget (`safety.tracking_error_max_rad`). Callers that need a
/// different budget — e.g. the boot orchestrator with
/// `safety.boot_tracking_error_max_rad` — should call
/// [`run_with_tracking_budget`] or [`run_with_overrides`] directly.
///
/// Resolves nominal speed via [`resolve_homing_speed`] inside [`run_with_tracking_budget`].
pub async fn run(
    state: SharedState,
    motor: Actuator,
    from_rad: f32,
    target_rad: f32,
) -> Result<(f32, u32), (String, f32)> {
    let budget = state.cfg.safety.tracking_error_max_rad;
    run_with_tracking_budget(state, motor, from_rad, target_rad, budget).await
}

/// After a successful home-ramp loop: RS03 **MIT spring-damper hold**
/// (`run_mode = 0` + single OperationCtrl frame with `vel = 0`,
/// `torque_ff = 0`, `kp`/`kd` from `safety`). The firmware then closes the
/// loop on encoder + the standing kp/kd alone — no streamed setpoint, no
/// continuous current draw, no servo whine — but the joint still resists
/// droop and snaps back when nudged.
///
/// Verifies telemetry after 500 ms; on verification failure, `cmd_stop`
/// and `mark_stopped`.
async fn finish_home_success(
    state: &SharedState,
    motor: &Actuator,
    role: &str,
    target_rad: f32,
    cfg: &SafetyConfig,
    last_measured: f32,
) -> Result<(), (String, f32)> {
    let target_p = PrincipalAngle::from_wrapped_rad(target_rad);
    let kp = cfg.hold_kp_nm_per_rad;
    let kd = cfg.hold_kd_nm_s_per_rad;
    if let Some(core) = state.real_can.clone() {
        let motor_owned = motor.clone();
        let t = target_p;
        match tokio::task::spawn_blocking(move || core.set_mit_hold(&motor_owned, t, kp, kd)).await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if let Some(c2) = state.real_can.clone() {
                    let motor_st = motor.clone();
                    let _ = tokio::task::spawn_blocking(move || c2.stop(&motor_st)).await;
                }
                state.mark_stopped(role);
                return Err((format!("can_command_failed: {e:#}"), last_measured));
            }
            Err(e) => return Err((format!("internal: spawn_blocking: {e}"), last_measured)),
        }
    }
    // `position_hold` is the runtime "drive is held in some non-velocity mode" flag —
    // it gates `Cmd::SetVelocity`'s smart re-arm so the next jog correctly issues
    // RUN_MODE=2 + cmd_enable. The bookkeeping is identical for PP and MIT holds.
    state.mark_stopped(role);
    state.mark_position_hold(role);

    tokio::time::sleep(Duration::from_millis(500)).await;

    let now_ms = Utc::now().timestamp_millis();
    let max_age_ms = cfg.tracking_freshness_max_age_ms as i64;
    let latest = state
        .latest
        .read()
        .expect("latest poisoned")
        .get(role)
        .cloned();
    let (mech_pos, age_ms) = match latest {
        Some(fb) => (fb.mech_pos_rad, now_ms - fb.t_ms),
        None => {
            warn!(
                role = %role,
                target_principal = target_p.raw(),
                "home_ramp: hold verification stale telemetry (missing)"
            );
            if let Some(core) = state.real_can.clone() {
                let motor_owned = motor.clone();
                let _ = tokio::task::spawn_blocking(move || core.stop(&motor_owned)).await;
            }
            state.mark_stopped(role);
            return Err(("hold_verification_stale_telemetry".into(), last_measured));
        }
    };
    if age_ms > max_age_ms {
        warn!(
            role = %role,
            age_ms,
            max_age_ms,
            target_principal = target_p.raw(),
            "home_ramp: hold verification stale telemetry (age)"
        );
        if let Some(core) = state.real_can.clone() {
            let motor_owned = motor.clone();
            let _ = tokio::task::spawn_blocking(move || core.stop(&motor_owned)).await;
        }
        state.mark_stopped(role);
        return Err(("hold_verification_stale_telemetry".into(), mech_pos));
    }
    let err = shortest_signed_delta(mech_pos, target_p.raw()).abs();
    let limit = cfg.target_tolerance_rad * 2.0;
    if err >= limit {
        warn!(
            role = %role,
            mech_pos,
            target_principal = target_p.raw(),
            err,
            limit,
            "home_ramp: hold verification failed"
        );
        if let Some(core) = state.real_can.clone() {
            let motor_owned = motor.clone();
            let _ = tokio::task::spawn_blocking(move || core.stop(&motor_owned)).await;
        }
        state.mark_stopped(role);
        return Err(("hold_verification_failed".into(), mech_pos));
    }
    Ok(())
}

pub fn resolve_homing_speed(state: &SharedState, motor: &Actuator) -> (f32, &'static str) {
    if let Some(v) = motor.common.homing_speed_rad_s {
        if v.is_finite() && v > 0.0 {
            return (v.min(MAX_HOMER_VEL_RAD_S), "actuator_override");
        }
    }
    let g = state.cfg.safety.effective_homing_speed_rad_s();
    let src = if state.cfg.safety.homing_speed_rad_s.is_some() {
        "global_config"
    } else {
        "derived_step_tick"
    };
    (g, src)
}

/// Home-ramp with the operator-typical tracking-error budget and speed resolved
/// from inventory / global config ([`resolve_homing_speed`] → [`run_with_overrides`]).
///
/// The budget overrides `safety.tracking_error_max_rad` for the life of this
/// run; other timing knobs come from `safety`.
pub async fn run_with_tracking_budget(
    state: SharedState,
    motor: Actuator,
    from_rad: f32,
    target_rad: f32,
    tracking_error_max_rad: f32,
) -> Result<(f32, u32), (String, f32)> {
    let (homing_speed_rad_s, homing_speed_source) = resolve_homing_speed(&state, &motor);
    run_with_overrides(
        state,
        motor,
        from_rad,
        target_rad,
        tracking_error_max_rad,
        homing_speed_rad_s,
        homing_speed_source,
    )
    .await
}

/// Home-ramp with explicit resolved nominal speed and tracking-error budget.
///
/// `homing_speed_rad_s` is clamped to [`MAX_HOMER_VEL_RAD_S`]. Per-tick step
/// size is `nominal_speed × tick_interval_s` (not `safety.step_size_rad` read
/// directly). [`run_with_tracking_budget`] and the boot orchestrator normally
/// pass the tuple from [`resolve_homing_speed`].
pub async fn run_with_overrides(
    state: SharedState,
    motor: Actuator,
    from_rad: f32,
    target_rad: f32,
    tracking_error_max_rad: f32,
    homing_speed_rad_s: f32,
    homing_speed_source: &'static str,
) -> Result<(f32, u32), (String, f32)> {
    let role = motor.common.role.clone();
    let cfg = state.cfg.safety.clone();
    let tick = Duration::from_millis(cfg.tick_interval_ms.max(5) as u64);
    let timeout = Duration::from_millis(cfg.homer_timeout_ms.max(1_000) as u64);
    let grace_ticks = cfg.tracking_error_grace_ticks;

    // Snapshot of `travel_limits` taken once at entry. Used **only** for
    // the predictive band-edge velocity cap (see `band_edge_distance`).
    // The per-tick band check below still re-reads the live inventory
    // via `enforce_position_with_path` so a config change mid-ramp
    // (`PUT /api/motors/:role/limits`) still aborts cleanly. The cap is
    // an opportunistic safety net — being one config-version stale on
    // it can at worst either mildly under-protect (limits widened, motor
    // decelerates more aggressively than necessary) or mildly over-cap
    // (limits tightened, but the live `enforce_position_with_path`
    // will still abort on actual band crossings); it can't make the
    // homer drive past the live limits.
    let limits_snapshot = motor.common.travel_limits.clone();

    let tick_secs = (cfg.tick_interval_ms.max(5) as f32) / 1000.0;
    let nominal_speed = homing_speed_rad_s.min(MAX_HOMER_VEL_RAD_S);
    let step_size_rad = nominal_speed * tick_secs;

    // Resolve the operator's target into the same unwrapped frame the
    // multi-turn encoder reports. The principal-angle delta is the
    // shortest signed path from current to wrap-to-pi(target); adding
    // it to the *unwrapped* current position gives the equivalent
    // unwrapped target. Without this step, asking to home a motor that
    // reads 6.299 rad to "0.0" would drive a full revolution
    // backwards.
    let signed_delta = shortest_signed_delta(from_rad, target_rad);
    let unwrapped_target = from_rad + signed_delta;

    let start = Instant::now();
    let mut setpoint_unwrapped = from_rad;
    let mut ticks: u32 = 0;
    let mut last_measured = from_rad;
    let homer_has_real_can = state.real_can.is_some();
    let debounce_ticks = cfg.tracking_error_debounce_ticks;
    let band_debounce_ticks = cfg.band_violation_debounce_ticks;
    let freshness_ms = cfg.tracking_freshness_max_age_ms as i64;
    let mut stale_stretch_logged = false;
    let mut consec_over: u32 = 0;
    let mut consec_band_over: u32 = 0;
    let mut consec_in_tolerance: u32 = 0;
    let dwell_need = cfg.target_dwell_ticks.max(1);

    // Trajectory bounds bookkeeping. Surfaced in the abort line so a
    // post-mortem can tell at a glance whether the motor (a) moved
    // toward target at all, (b) overshot, or (c) ran the wrong
    // direction (sign inversion between encoder and vel command). All
    // four start pinned to `from_rad` so a zero-tick run still shows
    // sensible values.
    let mut min_pos_seen = from_rad;
    let mut max_pos_seen = from_rad;
    let mut last_vel_commanded: f32 = 0.0;
    let mut total_can_sends: u32 = 0;
    let mut total_can_send_failures: u32 = 0;

    // Emit once-per-run: every knob and every starting condition that
    // could matter when post-mortem'ing a failure. Operators looking at
    // a `path_violation` or `tracking_error` later only need this
    // single line plus the matching exit-summary line below to
    // reconstruct what the homer was attempting and which budgets it
    // was operating against. Logged at info because (a) one line per
    // home is cheap, (b) every other "boot orchestrator: starting
    // auto-home" line in the journal will already be at info, so
    // matching the level keeps the chronology readable.
    info!(
        role = %role,
        from_rad,
        target_rad,
        signed_delta,
        unwrapped_target,
        limits_min_rad = limits_snapshot.as_ref().map(|l| l.min_rad),
        limits_max_rad = limits_snapshot.as_ref().map(|l| l.max_rad),
        tracking_error_max_rad,
        tracking_error_grace_ticks = grace_ticks,
        tracking_error_debounce_ticks = debounce_ticks,
        band_violation_debounce_ticks = band_debounce_ticks,
        step_size_rad,
        tick_interval_ms = cfg.tick_interval_ms,
        homing_speed_source,
        nominal_speed_rad_s = nominal_speed,
        target_tolerance_rad = cfg.target_tolerance_rad,
        homer_timeout_ms = cfg.homer_timeout_ms,
        tracking_freshness_max_age_ms = cfg.tracking_freshness_max_age_ms,
        has_real_can = homer_has_real_can,
        target_dwell_ticks = dwell_need,
        // Surface the resolved hold gains in the entry log so a
        // post-mortem can confirm config edits to
        // `safety.hold_kp_nm_per_rad` / `hold_kd_nm_s_per_rad` actually
        // loaded (cortex.toml is read once at process start; a stale
        // process serves stale gains until restarted). These values are
        // only consumed by `finish_home_success` after the loop
        // succeeds, but logging them here keeps the entry line as the
        // single authoritative dump of "what the homer believed when
        // it started", which is what operators reach for first.
        hold_kp_nm_per_rad = cfg.hold_kp_nm_per_rad,
        hold_kd_nm_s_per_rad = cfg.hold_kd_nm_s_per_rad,
        // `direction_sign` is applied at the CAN boundary
        // (`set_velocity_setpoint`, type-2/type-17 telemetry decode)
        // and is otherwise invisible to this loop, so the entry log
        // is the right (only) place to confirm it loaded as the
        // operator expected. A mistakenly-flipped sign turns a
        // healthy home into a tracking-error abort within ~150 ms;
        // having `direction_sign` next to `from_rad` / `target_rad`
        // / `signed_delta` makes that the very first thing a
        // post-mortem can rule in or out.
        direction_sign = motor.common.direction_sign,
        "home_ramp: starting"
    );

    // Periodic info-level progress log. Default tick is 50 ms, so
    // `progress_every_ticks = 20` emits one progress line per second.
    // Cheap enough to leave on at info during real homes (the longest
    // ones are bounded by `homer_timeout_ms`, default 30 s → ~30
    // lines), expensive enough to skip in the noisy unit-test flood.
    // Tests use a 5 ms tick + 5 s timeout, which would produce 200
    // periodic lines per stuck-motor test if we kept the same
    // interval, so we floor at "every 1 s of wall-clock if possible,
    // else every 200 ticks" — same idea, no per-test spam.
    let progress_every_ticks = (1_000 / cfg.tick_interval_ms.max(1)).max(20);

    let mut outcome = loop {
        if start.elapsed() >= timeout {
            break Err(("timeout".into(), last_measured));
        }
        ticks = ticks.saturating_add(1);

        let is_fresh = if homer_has_real_can {
            let now_ms = Utc::now().timestamp_millis();
            match state.latest.read().expect("latest poisoned").get(&role) {
                Some(fb) => {
                    let age_ms = now_ms - fb.t_ms;
                    if age_ms <= freshness_ms {
                        last_measured = fb.mech_pos_rad;
                        if stale_stretch_logged {
                            // Bookend the "stale telemetry" debug
                            // line so a post-mortem can measure the
                            // duration of every stretch from the log
                            // alone instead of guessing from gaps.
                            debug!(
                                role = %role,
                                tick = ticks,
                                age_ms,
                                "home_ramp: telemetry fresh again"
                            );
                        }
                        stale_stretch_logged = false;
                        true
                    } else {
                        if !stale_stretch_logged {
                            debug!(
                                role = %role,
                                tick = ticks,
                                age_ms,
                                max_age_ms = freshness_ms,
                                "home_ramp: stale telemetry, holding setpoint"
                            );
                            stale_stretch_logged = true;
                        }
                        false
                    }
                }
                None => {
                    if !stale_stretch_logged {
                        debug!(
                            role = %role,
                            tick = ticks,
                            "home_ramp: stale telemetry (missing), holding setpoint"
                        );
                        stale_stretch_logged = true;
                    }
                    false
                }
            }
        } else {
            true
        };

        let in_tolerance =
            shortest_signed_delta(last_measured, unwrapped_target).abs() < cfg.target_tolerance_rad;
        if is_fresh {
            if in_tolerance {
                consec_in_tolerance = consec_in_tolerance.saturating_add(1);
            } else {
                consec_in_tolerance = 0;
            }
        } else {
            consec_in_tolerance = 0;
        }

        // Dwell gate: require N consecutive fresh in-tolerance samples before
        // declaring success (see `SafetyConfig::target_dwell_ticks`).
        if (homer_has_real_can || ticks > 1) && is_fresh && consec_in_tolerance >= dwell_need {
            break Ok((last_measured, ticks));
        }

        // Ramp the setpoint only when telemetry is fresh (real CAN) or in
        // mock mode, so a stale `mech_pos_rad` cannot accumulate phantom
        // tracking error against a marching setpoint.
        if !homer_has_real_can || is_fresh {
            let remaining = unwrapped_target - setpoint_unwrapped;
            let step = remaining.signum() * remaining.abs().min(step_size_rad);
            setpoint_unwrapped += step;
        }

        let remaining = unwrapped_target - setpoint_unwrapped;

        // Re-check the path on principal angles so a config change
        // mid-ramp (or the motor drifting out of band under us)
        // aborts cleanly. Note: this check uses **live** inventory
        // (re-read every tick) so a `PUT /api/motors/:role/limits`
        // tightens the band on the very next iteration — the
        // `limits_snapshot` taken at entry is only consulted by the
        // predictive band-edge velocity cap below.
        let check = match enforce_position_with_path(
            &state,
            &role,
            UnwrappedAngle::new(last_measured),
            UnwrappedAngle::new(setpoint_unwrapped),
        ) {
            Ok(c) => c,
            Err(e) => break Err((format!("internal: {e:#}"), last_measured)),
        };
        let band_violation = matches!(
            check,
            BandCheck::OutOfBand { .. } | BandCheck::PathViolation { .. }
        );
        if band_violation_should_abort(
            homer_has_real_can,
            is_fresh,
            ticks,
            grace_ticks,
            band_violation,
            band_debounce_ticks,
            &mut consec_band_over,
            &role,
        ) {
            break Err(("path_violation".into(), last_measured));
        }
        // Mock-mode keeps the legacy single-sample abort: with
        // `homer_has_real_can = false` `band_violation_should_abort`
        // returns `false` for everything, so the existing contract
        // tests that pin a path-violation through a deliberately
        // out-of-band setpoint would otherwise loop until
        // `homer_timeout_ms`. Mirror that path explicitly here. On real
        // hardware this branch is dead because `homer_has_real_can`
        // controls both the early-return inside the gate and this
        // guard, so the gate is the only abort path that fires.
        if !homer_has_real_can && band_violation {
            break Err(("path_violation".into(), last_measured));
        }

        // Issue the velocity setpoint. We govern the magnitude by the
        // LARGER of the two remaining-distance measurements:
        //
        //   - `remaining` (target - setpoint): the trajectory's view.
        //     Drops to zero the tick the ramp arrives at the target.
        //   - `measured_remaining` (target - measured): the physical
        //     view. Stays non-zero until the motor actually parks at
        //     the target.
        //
        // Using `remaining` alone (the original implementation) made
        // the homer "feed-forward only": the moment the setpoint hit
        // the target, vel was commanded to zero — even if the motor
        // was still 2-3Â° short because the firmware velocity loop
        // tapered to a stall against gravity/static friction on the
        // final approach. The motor then sat in vel=0 hold mode
        // (audibly cogging) until `homer_timeout_ms` fired and the
        // homer gave up. By keeping `measured_remaining` in the mix,
        // we continue to push toward the target until the motor has
        // physically arrived (or the success-tolerance / timeout /
        // tracking-error checks fire). The `nominal_speed` cap and
        // the `approach_scale` taper keep the final approach soft.
        let measured_remaining = unwrapped_target - last_measured;
        let governing = if measured_remaining.abs() > remaining.abs() {
            measured_remaining
        } else {
            remaining
        };
        let direction = if governing.abs() < f32::EPSILON {
            0.0
        } else {
            governing.signum()
        };
        // Decompose the signed measured-vs-setpoint delta into the
        // two failure modes along the direction of motion:
        //
        //   `shortest_signed_delta(setpoint, measured)` returns
        //   `measured - setpoint` (modulo wrap). Multiplying by
        //   `direction` projects that onto the axis of motion:
        //
        //     lag     = motor BEHIND setpoint     (= -signed_errÂ·dir)
        //     overrun = motor AHEAD of setpoint   (= +signed_errÂ·dir)
        //
        // Lag is the binding/stall mode the tracking-error gate is
        // designed to catch — it feeds `err_rad` below. Overrun is
        // the gravity-assisted mode (shoulder_pitch / elbow_pitch
        // falling toward a low-gravity neutral pose under a payload —
        // firmware velocity-loop overshoot + gravity carry the motor
        // past the virtual setpoint each tick). It feeds `lag_scale`
        // a few lines down so the homer brakes commanded velocity as
        // the motor outruns the ramp instead of pinning Â±nominal_speed
        // and watching position lead grow unboundedly. Computed once
        // per tick from the same `last_measured` the band/vel logic
        // below references so all three derived scales (approach,
        // edge cap, lag) see a consistent snapshot.
        let signed_err = shortest_signed_delta(setpoint_unwrapped, last_measured);
        let overrun = (signed_err * direction).max(0.0);
        // Predictive band-edge velocity cap. The existing
        // `approach_scale = |governing| / step_size_rad` taper
        // decelerates as the motor approaches the **home target**, but
        // when the home target sits within ~`step_size_rad` of a band
        // edge (or, in the worst case, exactly at it), the residual
        // tapered velocity plus gravity / inertia carries the motor
        // past the edge before the reactive `governing` flip can
        // observe the overshoot on the next telemetry tick and reverse
        // course. By tapering against the smaller of "distance to
        // target" and "distance to band edge in the direction of
        // motion", commanded velocity now smoothly approaches zero as
        // the motor nears whichever boundary it would hit first. The
        // band-violation **debounce** above absorbs a residual
        // sub-step-size overshoot; this cap keeps that overshoot small
        // enough that the debounce window is genuinely sufficient
        // rather than a guess. See `band_edge_distance` for the math
        // and the no-limits short-circuit.
        let dist_to_edge = band_edge_distance(limits_snapshot.as_ref(), last_measured, direction);
        let governing_capped = governing.abs().min(dist_to_edge);
        let approach_scale = (governing_capped / step_size_rad.max(1e-6)).min(1.0);
        // Symmetric companion to `approach_scale`: when the motor is
        // racing ahead of the home-ramp trajectory (gravity assist on
        // a payload-loaded joint), taper commanded velocity toward
        // zero so the firmware velocity loop brakes the motor back
        // into trajectory. At an overrun of one `step_size_rad` we
        // command vel=0 and let the setpoint catch up; smaller
        // overruns get a linear ease-off. Combined with the one-sided
        // `tracking_error_should_abort` gate below — overrun never
        // counts toward the tracking abort — this keeps
        // gravity-assisted homes (shoulder_pitch falling toward its
        // low-gravity neutral pose) from either tripping a spurious
        // `tracking_error` abort OR running unboundedly past the
        // virtual setpoint into the band edge.
        let lag_scale = (1.0 - overrun / step_size_rad.max(1e-6)).clamp(0.0, 1.0);
        let mut vel = direction * nominal_speed * approach_scale * lag_scale;

        // Final-approach deadband: if the motor is already inside the
        // success tolerance, command zero velocity even if the early
        // success check above didn't fire (e.g. telemetry was stale on
        // *this* tick but `last_measured` is sticky from the most
        // recent fresh sample, which happens to be in-tolerance). This
        // is the symmetric companion to the early break: where that
        // exits the loop on fresh in-tolerance telemetry, this clamps
        // the commanded velocity on stale-but-likely-arrived
        // telemetry, so neither code path can keep nudging a parked
        // motor and re-introducing the limit cycle. Uses the same
        // shortest-signed-delta the success check uses so a measured
        // value on the other side of a wrap from `unwrapped_target`
        // still counts as in-tolerance.
        if shortest_signed_delta(last_measured, unwrapped_target).abs() < cfg.target_tolerance_rad {
            vel = 0.0;
        }

        // The band-edge cap is the most diagnostic single piece of
        // information for "is fix #2 actually doing anything?" — when
        // it's the binding constraint (i.e., the band edge is closer
        // than the home target), we know the homer would have
        // commanded a faster velocity but for the cap, and the
        // operator can correlate this with the absence of
        // `path_violation` aborts. Logged at debug so it doesn't spam
        // operator logs in steady state, but available with
        // `RUST_LOG=cortex::can::home_ramp=debug` whenever a homing
        // anomaly needs investigating.
        let edge_capped = dist_to_edge.is_finite() && dist_to_edge < governing.abs();
        if edge_capped {
            debug!(
                role = %role,
                tick = ticks,
                dist_to_edge,
                governing_abs = governing.abs(),
                vel,
                last_measured,
                "home_ramp: velocity capped by band edge (not by target distance)"
            );
        }

        if let Some(core) = state.real_can.clone() {
            let motor_for_blocking = motor.clone();
            let send = tokio::task::spawn_blocking(move || {
                core.set_velocity_setpoint(&motor_for_blocking, vel)
            })
            .await;
            match send {
                Ok(Ok(())) => {
                    total_can_sends = total_can_sends.saturating_add(1);
                }
                Ok(Err(e)) => {
                    total_can_send_failures = total_can_send_failures.saturating_add(1);
                    break Err((format!("can_command_failed: {e:#}"), last_measured));
                }
                Err(e) => {
                    break Err((format!("internal: spawn_blocking: {e}"), last_measured));
                }
            }
        }
        // Tick bookkeeping for the abort post-mortem. Updated AFTER the
        // CAN send so `last_vel_commanded` reflects the most recent
        // value the firmware actually saw (or attempted to see).
        // `min/max_pos_seen` track the trajectory bounds so the abort
        // line can answer "which direction did the joint actually
        // travel?" without needing per-tick debug logs.
        last_vel_commanded = vel;
        if last_measured < min_pos_seen {
            min_pos_seen = last_measured;
        }
        if last_measured > max_pos_seen {
            max_pos_seen = last_measured;
        }

        // Per-tick trace at debug. Ten-ish fields is a lot for one
        // line, but every field here has been wished for at least
        // once in past homing post-mortems — `is_fresh` tells you
        // whether the tracking-error gate ran this tick, the two
        // `consec_*` counters tell you how close we are to a debounce
        // trip, `edge_capped` tells you whether the band-edge cap is
        // the active limiter, and the position/setpoint/vel triple
        // lets you reconstruct the trajectory across a stretch of
        // log. Cheap enough at debug; off by default.
        debug!(
            role = %role,
            tick = ticks,
            is_fresh,
            last_measured,
            setpoint = setpoint_unwrapped,
            remaining,
            governing,
            direction,
            dist_to_edge,
            edge_capped,
            approach_scale,
            overrun,
            lag_scale,
            vel,
            consec_band_over,
            consec_in_tolerance,
            consec_tracking_over = consec_over,
            "home_ramp: tick"
        );

        // Periodic info-level progress so operators following along
        // in the journal see the homer make headway without enabling
        // debug. One line per ~1 second of wall-clock; cheap because
        // the home-ramp itself is bounded by `homer_timeout_ms`.
        if ticks.is_multiple_of(progress_every_ticks) {
            info!(
                role = %role,
                tick = ticks,
                last_measured,
                target_rad = unwrapped_target,
                distance_remaining = (unwrapped_target - last_measured).abs(),
                vel,
                edge_capped,
                "home_ramp: progress"
            );
        }

        // Mock mode: perfect tracking so contract tests pin the success path.
        if !homer_has_real_can {
            last_measured = setpoint_unwrapped;
        }

        // One-sided: only the LAG portion of the signed error feeds
        // the abort gate (see the lag/overrun decomposition near the
        // velocity computation above). Overrun is bounded by
        // `lag_scale` and is not a tracking failure — it just means
        // gravity is doing some of the work and the firmware velocity
        // loop will catch up on the next tick. Re-derive from the
        // SAME `last_measured` mock-mode may have just rewritten (so
        // mock mode still sees err_rad == 0 and the gate's mock-mode
        // short-circuit remains the authoritative no-op for tests).
        let err_rad =
            (-shortest_signed_delta(setpoint_unwrapped, last_measured) * direction).max(0.0);
        if tracking_error_should_abort(
            homer_has_real_can,
            is_fresh,
            ticks,
            grace_ticks,
            err_rad,
            tracking_error_max_rad,
            debounce_ticks,
            &mut consec_over,
            &role,
        ) {
            // Emit a focused info-level snapshot of the failing tick
            // so a tracking_error post-mortem doesn't need
            // RUST_LOG=debug. Pairs the in-loop control state with
            // the trajectory bounds tracked across the whole run, so
            // the operator can answer at a glance:
            //   - did the motor move toward or away from target?
            //     (compare `from_rad`, `min_pos_seen`, `max_pos_seen`,
            //     `last_measured` against `target_rad` in the
            //     bookend abort line)
            //   - was the velocity command sane and non-zero?
            //     (`last_vel_commanded`, `direction`)
            //   - is there a sign inversion? (direction toward target
            //     vs. observed motion direction)
            //   - did CAN actually send? (`total_can_sends` vs
            //     `ticks` in the abort line)
            info!(
                role = %role,
                tick = ticks,
                last_measured,
                setpoint = setpoint_unwrapped,
                target = unwrapped_target,
                from_rad,
                min_pos_seen,
                max_pos_seen,
                signed_err_to_setpoint = -shortest_signed_delta(setpoint_unwrapped, last_measured),
                err_rad_lag = err_rad,
                budget = tracking_error_max_rad,
                vel = last_vel_commanded,
                direction,
                approach_scale,
                lag_scale,
                overrun,
                edge_capped,
                dist_to_edge,
                total_can_sends,
                "home_ramp: tracking_error trip — final tick snapshot"
            );
            break Err(("tracking_error".into(), last_measured));
        }

        tokio::time::sleep(tick).await;
    };

    // Success: profile-position hold + verification. Failure/timeout: stop + clear enabled.
    match &outcome {
        Ok((final_pos, _ticks_done)) => {
            if let Err(e) =
                finish_home_success(&state, &motor, &role, target_rad, &cfg, *final_pos).await
            {
                outcome = Err(e);
            }
        }
        Err(_) => {
            if let Some(core) = state.real_can.clone() {
                let motor_for_stop = motor.clone();
                let _ = tokio::task::spawn_blocking(move || core.stop(&motor_for_stop)).await;
            }
            state.mark_stopped(&role);
        }
    }

    // Bookend the entry log. Same `role` + `target` so log filters
    // pair them up automatically; success and failure go through
    // separate arms so the structured fields are typed appropriately
    // (no `Option<String>` reason field cluttering the success line).
    // Both paths include the same locals we'd want during a
    // post-mortem: ticks, elapsed_ms, where the motor ended up, where
    // the setpoint ended up, distance to target, and how close we
    // came to either debounce trip. The orchestrator and operator
    // home handlers each emit their own outcome lines too — those
    // capture the *domain* meaning (boot orchestrator marked Homed,
    // operator received 200 OK), where this one captures the
    // *control loop* meaning.
    let elapsed_ms = start.elapsed().as_millis() as u64;
    match &outcome {
        Ok((final_pos, ticks_done)) => {
            info!(
                role = %role,
                final_pos_rad = final_pos,
                target_rad = unwrapped_target,
                distance_remaining = (unwrapped_target - final_pos).abs(),
                ticks = ticks_done,
                elapsed_ms,
                consec_band_over_at_exit = consec_band_over,
                consec_tracking_over_at_exit = consec_over,
                "home_ramp: completed"
            );
        }
        Err((reason, last_pos)) => {
            // Trajectory bounds + CAN send accounting are surfaced
            // here (alongside the per-trip snapshot at the abort
            // site) so the abort line by itself answers the most
            // common post-mortem questions:
            //   - did the joint move at all? (min/max vs from_rad)
            //   - which direction? (sign of (last_pos - from_rad)
            //     vs sign of (target - from_rad); if opposite, suspect
            //     sign inversion in the encoder/motor pairing)
            //   - did CAN sends keep up with ticks? (sends vs ticks)
            //   - what was the last commanded velocity? (vel = 0 at
            //     exit suggests we deadbanded ourselves silent against
            //     a stuck encoder)
            let traveled = last_pos - from_rad;
            let intended = unwrapped_target - from_rad;
            let direction_consistent = traveled * intended >= 0.0;
            info!(
                role = %role,
                reason = %reason,
                last_pos_rad = last_pos,
                from_rad,
                target_rad = unwrapped_target,
                setpoint_at_exit = setpoint_unwrapped,
                distance_remaining = (unwrapped_target - last_pos).abs(),
                traveled_rad = traveled,
                intended_rad = intended,
                direction_consistent,
                min_pos_seen,
                max_pos_seen,
                last_vel_commanded,
                total_can_sends,
                total_can_send_failures,
                limits_min_rad = limits_snapshot.as_ref().map(|l| l.min_rad),
                limits_max_rad = limits_snapshot.as_ref().map(|l| l.max_rad),
                ticks,
                elapsed_ms,
                consec_band_over_at_exit = consec_band_over,
                consec_tracking_over_at_exit = consec_over,
                "home_ramp: aborted"
            );
        }
    }

    outcome
}

#[cfg(test)]
#[path = "home_ramp_tracking_gate_tests.rs"]
mod tracking_gate_tests;

#[cfg(test)]
#[path = "home_ramp_band_gate_tests.rs"]
mod band_gate_tests;

#[cfg(test)]
#[path = "home_ramp_band_edge_distance_tests.rs"]
mod band_edge_distance_tests;

#[cfg(all(test, not(target_os = "linux")))]
#[path = "home_ramp_real_can_stub_tests.rs"]
mod real_can_stub_tests;

#[cfg(test)]
#[path = "home_ramp_dwell_tests.rs"]
mod dwell_tests;

#[cfg(test)]
#[path = "home_ramp_position_hold_tests.rs"]
mod position_hold_tests;
