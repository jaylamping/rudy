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
//!      consecutive **fresh** over-budget samples (post grace), path
//!      violation, or `homer_timeout_ms`.
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
use tracing::debug;

use crate::can::motion::shortest_signed_delta;
use crate::can::travel::{enforce_position_with_path, BandCheck};
use crate::inventory::Actuator;
use crate::state::SharedState;

/// Hard cap on the velocity the homer will issue. Matches the jog endpoint's
/// `MAX_JOG_VEL_RAD_S` so the homer can't outrun the operator-driven path.
/// In practice the per-tick rate (~0.4 rad/s with default `step_size_rad`
/// and `tick_interval_ms`) is well below this; the cap is a safety net.
pub const MAX_HOMER_VEL_RAD_S: f32 = 0.5;

/// Returns `true` when the homer should abort with `tracking_error`.
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
            "slow_ramp: tracking error accumulating"
        );
        *consec_over >= debounce_ticks
    } else {
        *consec_over = 0;
        false
    }
}

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
    motor: Actuator,
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
/// `target_tolerance_rad`, `tracking_error_grace_ticks`,
/// `tracking_freshness_max_age_ms`, `tracking_error_debounce_ticks`) come from
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
    motor: Actuator,
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
    let homer_has_real_can = state.real_can.is_some();
    let debounce_ticks = cfg.tracking_error_debounce_ticks;
    let freshness_ms = cfg.tracking_freshness_max_age_ms as i64;
    let mut stale_stretch_logged = false;
    let mut consec_over: u32 = 0;

    let outcome = loop {
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
                        stale_stretch_logged = false;
                        true
                    } else {
                        if !stale_stretch_logged {
                            debug!(
                                role = %role,
                                age_ms,
                                max_age_ms = freshness_ms,
                                "slow_ramp: stale telemetry, holding setpoint"
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
                            "slow_ramp: stale telemetry (missing), holding setpoint"
                        );
                        stale_stretch_logged = true;
                    }
                    false
                }
            }
        } else {
            true
        };

        // Ramp the setpoint only when telemetry is fresh (real CAN) or in
        // mock mode, so a stale `mech_pos_rad` cannot accumulate phantom
        // tracking error against a marching setpoint.
        if !homer_has_real_can || is_fresh {
            let remaining = unwrapped_target - setpoint_unwrapped;
            let step = remaining.signum() * remaining.abs().min(cfg.step_size_rad);
            setpoint_unwrapped += step;
        }

        let remaining = unwrapped_target - setpoint_unwrapped;

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
        // was still 2-3° short because the firmware velocity loop
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
        let approach_scale = (governing.abs() / cfg.step_size_rad.max(1e-6)).min(1.0);
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

        // Mock mode: perfect tracking so contract tests pin the success path.
        if !homer_has_real_can {
            last_measured = setpoint_unwrapped;
        }

        let err_rad = shortest_signed_delta(setpoint_unwrapped, last_measured).abs();
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
            break Err(("tracking_error".into(), last_measured));
        }

        // Success when we're within tolerance of the target. Also
        // compared via shortest signed delta so a measured value that
        // happens to land on the other side of a wrap from the
        // unwrapped_target still counts.
        if shortest_signed_delta(last_measured, unwrapped_target).abs() < cfg.target_tolerance_rad {
            break Ok((last_measured, ticks));
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

#[cfg(test)]
mod tracking_gate_tests {
    use super::tracking_error_should_abort;

    #[test]
    fn debounce_trips_on_third_consecutive_fresh_over_budget() {
        let mut c = 0;
        assert!(!tracking_error_should_abort(
            true, true, 4, 3, 0.06, 0.05, 3, &mut c, "m"
        ));
        assert_eq!(c, 1);
        assert!(!tracking_error_should_abort(
            true, true, 5, 3, 0.06, 0.05, 3, &mut c, "m"
        ));
        assert_eq!(c, 2);
        assert!(tracking_error_should_abort(
            true, true, 6, 3, 0.06, 0.05, 3, &mut c, "m"
        ));
        assert_eq!(c, 3);
    }

    #[test]
    fn single_good_sample_resets_debounce() {
        let mut c = 0;
        assert!(!tracking_error_should_abort(
            true, true, 4, 3, 0.06, 0.05, 3, &mut c, "m"
        ));
        assert_eq!(c, 1);
        assert!(!tracking_error_should_abort(
            true, true, 5, 3, 0.01, 0.05, 3, &mut c, "m"
        ));
        assert_eq!(c, 0);
        assert!(!tracking_error_should_abort(
            true, true, 6, 3, 0.06, 0.05, 3, &mut c, "m"
        ));
        assert_eq!(c, 1);
    }

    #[test]
    fn stale_tick_leaves_consec_unchanged() {
        let mut c = 2;
        assert!(!tracking_error_should_abort(
            true, false, 10, 0, 1.0, 0.05, 3, &mut c, "m"
        ));
        assert_eq!(c, 2);
    }

    #[test]
    fn mock_mode_skips_tracking_abort() {
        let mut c = 0;
        assert!(!tracking_error_should_abort(
            false, true, 100, 0, 10.0, 0.05, 3, &mut c, "m"
        ));
        assert_eq!(c, 0);
    }

    #[test]
    fn grace_suppresses_tracking_abort() {
        let mut c = 0;
        assert!(!tracking_error_should_abort(
            true, true, 2, 3, 10.0, 0.05, 1, &mut c, "m"
        ));
        assert_eq!(c, 0);
    }
}

/// `slow_ramp` with `real_can = Some` only builds on non-Linux CI hosts using
/// the in-tree stub (`set_velocity_setpoint` / `stop` are no-op `Ok`).
#[cfg(all(test, not(target_os = "linux")))]
mod real_can_stub_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::run_with_tracking_budget;
    use crate::audit::AuditLog;
    use crate::can;
    use crate::config::{
        CanConfig, Config, HttpConfig, LogsConfig, PathsConfig, SafetyConfig, TelemetryConfig,
        WebTransportConfig,
    };
    use crate::inventory::Inventory;
    use crate::reminders::ReminderStore;
    use crate::spec;
    use crate::state::AppState;
    use crate::types::MotorFeedback;

    fn state_with_real_can_stub() -> (crate::state::SharedState, crate::inventory::Actuator) {
        let dir = tempfile::tempdir().unwrap();
        let spec_path = dir.path().join("robstride_rs03.yaml");
        std::fs::write(
            &spec_path,
            "schema_version: 2\nactuator_model: RS03\nfirmware_limits: {}\nobservables: {}\n",
        )
        .unwrap();
        let inv_path = dir.path().join("inv.yaml");
        std::fs::write(
            &inv_path,
            "schema_version: 2\ndevices:\n  - kind: actuator\n    role: m\n    can_bus: can0\n    can_id: 1\n    present: true\n    family:\n      kind: robstride\n      model: rs03\n    travel_limits:\n      min_rad: -1.0\n      max_rad: 1.0\n",
        )
        .unwrap();
        let cfg = Config {
            http: HttpConfig {
                bind: "127.0.0.1:0".into(),
            },
            webtransport: WebTransportConfig {
                bind: "127.0.0.1:0".into(),
                enabled: false,
                cert_path: None,
                key_path: None,
            },
            paths: PathsConfig {
                actuator_spec: spec_path.clone(),
                inventory: inv_path.clone(),
                inventory_seed: None,
                audit_log: dir.path().join("audit.jsonl"),
            },
            can: CanConfig {
                mock: true,
                buses: vec![],
            },
            telemetry: TelemetryConfig {
                poll_interval_ms: 10,
            },
            safety: SafetyConfig {
                require_verified: false,
                boot_max_step_rad: 0.087,
                step_size_rad: 0.02,
                tick_interval_ms: 5,
                tracking_error_max_rad: 0.05,
                tracking_error_grace_ticks: 0,
                tracking_freshness_max_age_ms: 100,
                tracking_error_debounce_ticks: 3,
                boot_tracking_error_max_rad: 0.05,
                target_tolerance_rad: 0.005,
                homer_timeout_ms: 5_000,
                max_feedback_age_ms: 100,
                commission_readback_tolerance_rad: 1e-3,
                auto_home_on_boot: true,
                scan_on_boot: true,
            },
            logs: LogsConfig {
                db_path: dir.path().join("logs.db"),
                ..LogsConfig::default()
            },
        };
        let specs = spec::load_robstride_specs(dir.path(), Some(&spec_path)).unwrap();
        let inv = Inventory::load(&inv_path).unwrap();
        let motor = inv.actuators().next().cloned().expect("fixture actuator");
        let audit = AuditLog::open(dir.path().join("audit.jsonl")).unwrap();
        let real_can = Some(Arc::new(can::RealCanHandle));
        let reminders = ReminderStore::open(dir.path().join("reminders.json")).unwrap();
        let state = Arc::new(AppState::new(cfg, specs, inv, audit, real_can, reminders));
        (state, motor)
    }

    #[tokio::test]
    async fn stuck_motor_aborts_after_debounced_tracking_error() {
        let (state, motor) = state_with_real_can_stub();
        let role = motor.common.role.clone();
        {
            let mut latest = state.latest.write().unwrap();
            latest.insert(
                role.clone(),
                MotorFeedback {
                    t_ms: chrono::Utc::now().timestamp_millis(),
                    role: role.clone(),
                    can_id: 1,
                    mech_pos_rad: 0.0,
                    mech_vel_rad_s: 0.0,
                    torque_nm: 0.0,
                    vbus_v: 48.0,
                    temp_c: 30.0,
                    fault_sta: 0,
                    warn_sta: 0,
                },
            );
        }
        let updater = {
            let state = state.clone();
            let role = role.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                    let mut w = state.latest.write().unwrap();
                    if let Some(fb) = w.get_mut(&role) {
                        fb.t_ms = chrono::Utc::now().timestamp_millis();
                        fb.mech_pos_rad = 0.0;
                    }
                }
            })
        };
        let r = run_with_tracking_budget(state.clone(), motor, 0.0, 0.5, 0.05).await;
        updater.abort();
        let Err((reason, _)) = r else {
            panic!("expected Err, got {r:?}");
        };
        assert_eq!(reason, "tracking_error");
    }

    #[tokio::test]
    async fn stale_telemetry_hold_then_fresh_run_completes() {
        let (state, motor) = state_with_real_can_stub();
        let role = motor.common.role.clone();
        let stale_ms = chrono::Utc::now().timestamp_millis() - 60_000;
        {
            let mut latest = state.latest.write().unwrap();
            latest.insert(
                role.clone(),
                MotorFeedback {
                    t_ms: stale_ms,
                    role: role.clone(),
                    can_id: 1,
                    mech_pos_rad: 0.0,
                    mech_vel_rad_s: 0.0,
                    torque_nm: 0.0,
                    vbus_v: 48.0,
                    temp_c: 30.0,
                    fault_sta: 0,
                    warn_sta: 0,
                },
            );
        }
        let state2 = state.clone();
        let role2 = role.clone();
        let updater = tokio::spawn(async move {
            let mut phase2 = false;
            let t0 = tokio::time::Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(3)).await;
                if t0.elapsed() > Duration::from_millis(50) {
                    phase2 = true;
                }
                let mut w = state2.latest.write().unwrap();
                let fb = w.get_mut(&role2).unwrap();
                if phase2 {
                    fb.t_ms = chrono::Utc::now().timestamp_millis();
                    fb.mech_pos_rad = (fb.mech_pos_rad + 0.03).min(0.25);
                }
            }
        });
        let r = run_with_tracking_budget(state.clone(), motor, 0.0, 0.12, 0.05).await;
        updater.abort();
        assert!(r.is_ok(), "expected Ok, got {r:?}");
    }
}
