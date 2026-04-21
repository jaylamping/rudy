//! Slow-ramp closed loop that walks a motor from a current position to a
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
//! Behavior is unchanged from the pre-refactor `run_homer`: each tick
//!   1. Re-runs the path-aware band check on the current measured
//!      position vs. the next setpoint.
//!   2. Issues a velocity setpoint sized so the motor advances by
//!      ~`step_size_rad` per `tick_interval_ms` (default ~0.4 rad/s ≈
//!      23 deg/s), in the direction of the remaining signed delta.
//!   3. Reads the latest type-2 telemetry row from `state.latest`.
//!   4. Aborts on tracking error (motor not following), path violation
//!      (config edited mid-move, or measured drifted out of band), or
//!      `homer_timeout_ms`.
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

use crate::can::motion::shortest_signed_delta;
use crate::can::travel::{enforce_position_with_path, BandCheck};
use crate::inventory::Motor;
use crate::state::SharedState;

/// Hard cap on the velocity the homer will issue. Matches the jog endpoint's
/// `MAX_JOG_VEL_RAD_S` so the homer can't outrun the operator-driven path.
/// In practice the per-tick rate (~0.4 rad/s with default `step_size_rad`
/// and `tick_interval_ms`) is well below this; the cap is a safety net.
pub const MAX_HOMER_VEL_RAD_S: f32 = 0.5;

/// Slow-ramp closed loop. See module docstring for the full semantics.
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
/// different budget — currently just the boot orchestrator, which
/// drives cold motors at boot and warrants more headroom — should
/// call [`run_with_tracking_budget`] directly.
pub async fn run(
    state: SharedState,
    motor: Motor,
    from_rad: f32,
    target_rad: f32,
) -> Result<(f32, u32), (String, f32)> {
    let budget = state.cfg.safety.tracking_error_max_rad;
    run_with_tracking_budget(state, motor, from_rad, target_rad, budget).await
}

/// Slow-ramp closed loop with a caller-supplied tracking-error budget.
///
/// The budget overrides `safety.tracking_error_max_rad` for the life of
/// this run; it does NOT mutate config. All other knobs
/// (`step_size_rad`, `tick_interval_ms`, `homer_timeout_ms`,
/// `target_tolerance_rad`, `tracking_error_grace_ticks`) come from
/// `safety`.
///
/// Use this entry point when the caller has a principled reason to
/// loosen (or tighten) the operator-driven default. Today the only
/// caller is [`crate::boot_orchestrator::maybe_run`], which passes
/// `safety.boot_tracking_error_max_rad` because the orchestrator runs
/// unattended on cold motors at boot and a ~3° budget falsely aborts
/// every time.
pub async fn run_with_tracking_budget(
    state: SharedState,
    motor: Motor,
    from_rad: f32,
    target_rad: f32,
    tracking_error_max_rad: f32,
) -> Result<(f32, u32), (String, f32)> {
    let role = motor.common.role.clone();
    let cfg = state.cfg.safety.clone();
    let tick = Duration::from_millis(cfg.tick_interval_ms.max(5) as u64);
    let timeout = Duration::from_millis(cfg.homer_timeout_ms.max(1_000) as u64);
    let grace_ticks = cfg.tracking_error_grace_ticks;

    // Effective top speed: one `step_size_rad` per `tick_interval_ms`,
    // clamped to MAX_HOMER_VEL_RAD_S as a hard upper bound. With the
    // defaults (0.02 rad / 50 ms) this works out to ~0.4 rad/s.
    let tick_secs = (cfg.tick_interval_ms.max(5) as f32) / 1000.0;
    let nominal_speed = (cfg.step_size_rad / tick_secs).min(MAX_HOMER_VEL_RAD_S);

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

    let outcome = loop {
        if start.elapsed() >= timeout {
            break Err(("timeout".into(), last_measured));
        }
        ticks = ticks.saturating_add(1);

        // Ramp the setpoint by at most `step_size_rad` toward the
        // unwrapped target. We carry the unwrapped value so the
        // tracking-error check below can compare against the raw
        // (multi-turn) measured position without losing a revolution.
        let remaining = unwrapped_target - setpoint_unwrapped;
        let step = remaining.signum() * remaining.abs().min(cfg.step_size_rad);
        setpoint_unwrapped += step;

        // Re-check the path on principal angles so a config change
        // mid-ramp (or the motor drifting out of band under us)
        // aborts cleanly.
        let check =
            match enforce_position_with_path(&state, &role, last_measured, setpoint_unwrapped) {
                Ok(c) => c,
                Err(e) => break Err((format!("internal: {e:#}"), last_measured)),
            };
        if let BandCheck::OutOfBand { .. } | BandCheck::PathViolation { .. } = check {
            break Err(("path_violation".into(), last_measured));
        }

        // Issue the velocity setpoint. Direction: sign of remaining;
        // magnitude: nominal_speed scaled down on the final approach
        // so we don't overshoot the target tolerance.
        let direction = if remaining.abs() < f32::EPSILON {
            0.0
        } else {
            remaining.signum()
        };
        let approach_scale = (remaining.abs() / cfg.step_size_rad.max(1e-6)).min(1.0);
        let vel = direction * nominal_speed * approach_scale;
        if let Some(core) = state.real_can.clone() {
            let motor_for_blocking = motor.clone();
            let send = tokio::task::spawn_blocking(move || {
                core.set_velocity_setpoint(&motor_for_blocking, vel)
            })
            .await;
            match send {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    break Err((format!("can_command_failed: {e:#}"), last_measured));
                }
                Err(e) => {
                    break Err((format!("internal: spawn_blocking: {e}"), last_measured));
                }
            }
        }

        // Read measured. In mock mode (no real CAN) we simulate
        // perfect tracking so the existing tests still pin the success
        // path without standing up a full bus.
        let measured = if state.real_can.is_some() {
            state
                .latest
                .read()
                .expect("latest poisoned")
                .get(&role)
                .map(|f| f.mech_pos_rad)
                .unwrap_or(last_measured)
        } else {
            setpoint_unwrapped
        };
        last_measured = measured;

        // Tracking-error check: compare the current ramp setpoint to
        // the freshly-measured position via shortest signed delta so
        // an N-turn unwrapped reading doesn't trip a false abort.
        //
        // Suppressed for the first `grace_ticks` ticks so a cold motor
        // (one that has just been re-armed by the bus_worker's
        // RUN_MODE + cmd_enable sequence and hasn't received a single
        // type-2 frame yet under the new velocity command) gets a
        // chance to start moving before the abort kicks in. Without
        // the grace window, the homer aborts on tick 2-3 every time —
        // the setpoint advances by `step_size_rad` per tick, but the
        // measurement lags by one full step until the firmware loop
        // and telemetry pipeline catch up. The `homer_timeout_ms`
        // ceiling still backstops a motor that genuinely refuses.
        if ticks > grace_ticks
            && shortest_signed_delta(setpoint_unwrapped, measured).abs() > tracking_error_max_rad
        {
            break Err(("tracking_error".into(), measured));
        }

        // Success when we're within tolerance of the target. Also
        // compared via shortest signed delta so a measured value that
        // happens to land on the other side of a wrap from the
        // unwrapped_target still counts.
        if shortest_signed_delta(measured, unwrapped_target).abs() < cfg.target_tolerance_rad {
            break Ok((measured, ticks));
        }

        tokio::time::sleep(tick).await;
    };

    // Always stop the motor before returning. Errors here are logged
    // but don't change the outcome — the watchdog and firmware
    // canTimeout backstop us if cmd_stop didn't reach the bus.
    if let Some(core) = state.real_can.clone() {
        let motor_for_stop = motor.clone();
        let _ = tokio::task::spawn_blocking(move || core.stop(&motor_for_stop)).await;
    }
    state.mark_stopped(&role);

    outcome
}
