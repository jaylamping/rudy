//! The per-motor closed-loop controller task.
//!
//! One [`Controller`] per running motion. Spawned by
//! [`crate::motion::registry::MotionRegistry::start`] and supervised by
//! the registry until the motor is stopped (operator, watchdog, fault,
//! or shutdown). The registry never touches the inner motion state —
//! it owns only the cancellation signal and the `intent` watch sender.
//!
//! Why a long-running task instead of a per-tick spawn:
//!
//! * The controller needs to keep velocity flowing on a deterministic
//!   cadence. The bus_worker's smart re-arm logic (cmd_stop → RUN_MODE →
//!   SPD_REF → cmd_enable on the first frame, SPD_REF only thereafter) is
//!   the reason a
//!   sustained server-side loop produces smooth motion in the first
//!   place — every task respawn would re-trip the re-arm cycle and
//!   reintroduce the jitter we're killing.
//! * Stop discipline collapses to a single point: every exit path of
//!   `run()` issues `cmd_stop` and clears `state.enabled`. Modeled on
//!   the existing exit gate in [`crate::api::home::run_homer`].
//!
//! The controller does NOT own a copy of the bus or talk to wtransport
//! directly; it routes outbound commands through [`drive_velocity`] or
//! [`drive_mit_stream`] (wrappers over `RealCanHandle` that no-op in mock
//! mode) and outbound status through [`crate::state::AppState::motion_status_tx`].

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{watch, Notify};
use tokio::time::interval;
use tracing::{debug, warn};

use crate::audit::{AuditEntry, AuditResult};
use crate::boot_state;
use crate::config::MotionBackend;
use crate::inventory::Actuator;
use crate::motion::intent::MotionIntent;
use crate::motion::mit;
use crate::motion::preflight::{PreflightChecks, PreflightFailure};
use crate::motion::smoothing::MitTargetSmoothing;
use crate::motion::status::{MotionState, MotionStatus, MotionStopReason};
use crate::motion::sweep::{self, SweepState};
use crate::motion::wave::{self, WaveState};
use crate::state::SharedState;

/// Minimum tick interval. Set to 10 ms to match the RS03's active-report
/// floor (`EPScan_time = 1`), so the controller can consume 100 Hz feedback
/// without aliasing.
const MIN_TICK_INTERVAL_MS: u64 = 10;

/// Default tick interval. Keep aligned with `MIN_TICK_INTERVAL_MS` so the
/// controller loop runs at the same 100 Hz cadence as firmware feedback.
const DEFAULT_TICK_INTERVAL_MS: u64 = 10;

/// Heartbeat window for [`MotionIntent::Jog`]. If the operator's
/// dead-man stream doesn't refresh within this many milliseconds, the
/// controller stops. Sized larger than one tick so a single dropped
/// packet doesn't cut a finger-held jog.
pub const JOG_HEARTBEAT_TTL_MS: u64 = 250;

/// All inputs the spawned task needs to drive one motion. Built by the
/// registry; the controller takes ownership.
pub struct ControllerTask {
    pub state: SharedState,
    pub motor: Actuator,
    pub run_id: String,
    pub intent_rx: watch::Receiver<MotionIntent>,
    pub stop: Arc<Notify>,
    /// Indicator the registry sets when a *different* request supersedes
    /// this one for the same role. Read on cancellation so the stop
    /// reason is `Superseded` instead of `Operator`.
    pub superseded: Arc<std::sync::atomic::AtomicBool>,
    /// Cancellation signaller fired when the daemon is shutting down.
    /// Currently unused (the registry just lets the runtime drop the
    /// task), but kept in the struct so the wiring story is uniform.
    pub shutdown: Arc<Notify>,
}

/// Drive one motion run to completion. Always issues `cmd_stop` and
/// emits a final `Stopped` status before returning.
pub async fn run(mut task: ControllerTask) {
    let role = task.motor.common.role.clone();
    let kind_str = task.intent_rx.borrow().kind_str();

    audit_start(&task);
    broadcast_running(&task, 0.0, task.intent_rx.borrow().clone(), 0.0);

    // Per-pattern accumulator state. Initialised lazily on the first
    // tick so we have live telemetry to derive the initial direction.
    let mut sweep_state: Option<SweepState> = None;
    let mut wave_state: Option<WaveState> = None;
    let mut mit_smooth = MitTargetSmoothing::default();

    // Heartbeat deadline for jog motions. Refreshed by
    // `MotionRegistry::heartbeat_jog`.
    let mut jog_deadline = std::time::Instant::now() + Duration::from_millis(JOG_HEARTBEAT_TTL_MS);

    let tick_interval_ms = DEFAULT_TICK_INTERVAL_MS.max(MIN_TICK_INTERVAL_MS);
    let mut tick = interval(Duration::from_millis(tick_interval_ms));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let stop_reason: MotionStopReason = loop {
        tokio::select! {
            biased;

            _ = task.stop.notified() => {
                if task.superseded.load(std::sync::atomic::Ordering::Acquire) {
                    break MotionStopReason::Superseded;
                }
                break MotionStopReason::Operator;
            }
            _ = task.shutdown.notified() => {
                break MotionStopReason::Shutdown;
            }
            changed = task.intent_rx.changed() => {
                if changed.is_err() {
                    // Registry dropped the intent_tx side; treat as client gone.
                    break MotionStopReason::ClientGone;
                }
                // On a jog intent update, refresh the heartbeat — the
                // operator clearly is still holding the jog.
                if matches!(*task.intent_rx.borrow(), MotionIntent::Jog { .. }) {
                    jog_deadline = std::time::Instant::now()
                        + Duration::from_millis(JOG_HEARTBEAT_TTL_MS);
                }
                continue;
            }
            _ = tick.tick() => {}
        }

        // Re-run the full preflight on every tick. Cheap (lock reads + a
        // few comparisons) and the only way to fail closed on
        // mid-run band edits, fault transitions, or telemetry stalls.
        let preflight = PreflightChecks {
            state: &task.state,
            role: &role,
            vel_rad_s: 0.0,
            horizon_ms: tick_interval_ms,
            target_position_rad: None,
        };
        let pf = match preflight.run() {
            Ok(pf) => pf,
            Err(e) => {
                break stop_reason_from_preflight(&e);
            }
        };

        let intent = task.intent_rx.borrow().clone();
        let limits_opt = task
            .state
            .inventory
            .read()
            .expect("inventory poisoned")
            .actuator_by_role(&role)
            .and_then(|m| m.common.travel_limits.clone());

        // Compute desired velocity from the per-pattern step function.
        let desired_vel = match &intent {
            MotionIntent::Sweep {
                speed_rad_s,
                turnaround_rad,
            } => match limits_opt.as_ref() {
                Some(limits) => {
                    let s = sweep_state.get_or_insert_with(|| {
                        SweepState::from_position(pf.feedback.mech_pos_rad, limits)
                    });
                    let (v, ns) = sweep::step(
                        pf.feedback.mech_pos_rad,
                        *s,
                        limits,
                        *speed_rad_s,
                        *turnaround_rad,
                    );
                    *s = ns;
                    v
                }
                None => {
                    // Sweep without travel limits is meaningless; refuse
                    // rather than free-running to the firmware envelope.
                    break MotionStopReason::TravelLimitViolation;
                }
            },
            MotionIntent::Wave {
                center_rad,
                amplitude_rad,
                speed_rad_s,
                turnaround_rad,
            } => {
                // For wave, fabricate a "limits" of the band (or the
                // hardware envelope when no band is set) so the step
                // function can apply its own clipping.
                let limits = limits_opt
                    .clone()
                    .unwrap_or(crate::inventory::TravelLimits {
                        min_rad: -std::f32::consts::PI,
                        max_rad: std::f32::consts::PI,
                        updated_at: None,
                    });
                let s = wave_state.get_or_insert_with(|| {
                    WaveState::from_position(pf.feedback.mech_pos_rad, *center_rad)
                });
                let (v, ns) = wave::step(
                    pf.feedback.mech_pos_rad,
                    *s,
                    &limits,
                    *center_rad,
                    *amplitude_rad,
                    *speed_rad_s,
                    *turnaround_rad,
                );
                *s = ns;
                v
            }
            MotionIntent::Jog { vel_rad_s } => {
                if std::time::Instant::now() >= jog_deadline {
                    break MotionStopReason::HeartbeatLapsed;
                }
                *vel_rad_s
            }
        };

        let (
            motion_backend,
            mit_lpf_cutoff_hz,
            mit_min_jerk_blend_ms,
            hold_kp,
            hold_kd,
            mit_max_step_global,
        ) = {
            let eff = task.state.read_effective();
            let s = &eff.safety;
            (
                s.motion_backend,
                s.mit_lpf_cutoff_hz,
                s.mit_min_jerk_blend_ms,
                s.hold_kp_nm_per_rad,
                s.hold_kd_nm_s_per_rad,
                s.mit_max_angle_step_rad,
            )
        };

        let dt_s = tick_interval_ms as f32 / 1000.0;

        match motion_backend {
            MotionBackend::Mit => {
                let raw_target = pf.feedback.mech_pos_rad + desired_vel * dt_s;
                let step_cap = mit::mit_step_max_rad_or(&task.motor, mit_max_step_global);
                let sm_in = mit_smooth.smooth(
                    pf.feedback.mech_pos_rad,
                    raw_target,
                    dt_s,
                    mit_lpf_cutoff_hz,
                    mit_min_jerk_blend_ms,
                );
                let mit_target = mit::clamp_mit_step(pf.feedback.mech_pos_rad, sm_in, step_cap);
                let projected_check = PreflightChecks {
                    state: &task.state,
                    role: &role,
                    vel_rad_s: 0.0,
                    horizon_ms: tick_interval_ms,
                    target_position_rad: Some(mit_target),
                }
                .run();
                if let Err(e) = projected_check {
                    break stop_reason_from_preflight(&e);
                }
                let (kp, kd) = mit::mit_command_kp_kd_or(&task.motor, hold_kp, hold_kd);
                if let Err(e) =
                    drive_mit_stream(&task.state, &task.motor, mit_target, 0.0, 0.0, kp, kd).await
                {
                    break MotionStopReason::BusError(e);
                }
                task.state.mark_enabled(&role);
                let eff_vel = if dt_s > 0.0 {
                    (mit_target - pf.feedback.mech_pos_rad) / dt_s
                } else {
                    0.0
                };
                broadcast_running(&task, pf.feedback.mech_pos_rad, intent, eff_vel);
            }
            MotionBackend::Velocity => {
                let projected_check = PreflightChecks {
                    state: &task.state,
                    role: &role,
                    vel_rad_s: desired_vel,
                    horizon_ms: tick_interval_ms,
                    target_position_rad: None,
                }
                .run();
                let vel = match projected_check {
                    Ok(_) => desired_vel,
                    Err(e) => break stop_reason_from_preflight(&e),
                };
                if let Err(e) = drive_velocity(&task.state, &task.motor, vel).await {
                    break MotionStopReason::BusError(e);
                }
                task.state.mark_enabled(&role);
                broadcast_running(&task, pf.feedback.mech_pos_rad, intent, vel);
            }
        }
    };

    // Always stop the motor before returning. Mirrors the exit
    // discipline in api::home::run_homer.
    let _ = drive_stop(&task.state, &task.motor).await;
    task.state.mark_stopped(&role);

    let final_pos = task
        .state
        .latest
        .read()
        .expect("latest poisoned")
        .get(&role)
        .map(|f| f.mech_pos_rad)
        .unwrap_or(0.0);

    broadcast_stopped(&task, final_pos, &stop_reason);
    audit_stop(&task, &stop_reason);

    if !matches!(stop_reason, MotionStopReason::Operator) {
        debug!(role = %role, kind = kind_str, reason = stop_reason.label(), "motion exited");
    }
}

fn stop_reason_from_preflight(e: &PreflightFailure) -> MotionStopReason {
    match e {
        PreflightFailure::StaleTelemetry { .. } | PreflightFailure::NoTelemetry => {
            MotionStopReason::StaleTelemetry
        }
        PreflightFailure::OutOfBand { .. }
        | PreflightFailure::PathViolation { .. }
        | PreflightFailure::StepTooLarge { .. } => MotionStopReason::TravelLimitViolation,
        PreflightFailure::BootOutOfBand { .. }
        | PreflightFailure::BootNotReady { .. }
        | PreflightFailure::UnknownActuator
        | PreflightFailure::Absent
        | PreflightFailure::NotVerified => MotionStopReason::BootStateLost,
        PreflightFailure::LimbQuarantined { .. } | PreflightFailure::SettingsRecovery => {
            MotionStopReason::BootStateLost
        }
        PreflightFailure::Internal(s) => MotionStopReason::BusError(s.clone()),
    }
}

async fn drive_mit_stream(
    state: &SharedState,
    motor: &Actuator,
    position_rad: f32,
    velocity_rad_s: f32,
    torque_ff_nm: f32,
    kp_nm_per_rad: f32,
    kd_nm_s_per_rad: f32,
) -> Result<(), String> {
    let Some(core) = state.real_can.clone() else {
        return Ok(());
    };
    let motor_for_blocking = motor.clone();
    tokio::task::spawn_blocking(move || {
        core.set_mit_command_stream(
            &motor_for_blocking,
            position_rad,
            velocity_rad_s,
            torque_ff_nm,
            kp_nm_per_rad,
            kd_nm_s_per_rad,
        )
    })
    .await
    .map_err(|e| format!("spawn_blocking: {e}"))?
    .map_err(|e| format!("{e:#}"))
}

async fn drive_velocity(state: &SharedState, motor: &Actuator, vel: f32) -> Result<(), String> {
    let Some(core) = state.real_can.clone() else {
        return Ok(());
    };
    let motor_for_blocking = motor.clone();
    tokio::task::spawn_blocking(move || core.set_velocity_setpoint(&motor_for_blocking, vel))
        .await
        .map_err(|e| format!("spawn_blocking: {e}"))?
        .map_err(|e| format!("{e:#}"))
}

async fn drive_stop(state: &SharedState, motor: &Actuator) -> Result<(), String> {
    let Some(core) = state.real_can.clone() else {
        return Ok(());
    };
    let motor_for_blocking = motor.clone();
    tokio::task::spawn_blocking(move || core.stop(&motor_for_blocking))
        .await
        .map_err(|e| format!("spawn_blocking: {e}"))?
        .map_err(|e| format!("{e:#}"))
}

fn broadcast_running(task: &ControllerTask, mech_pos_rad: f32, intent: MotionIntent, vel: f32) {
    let snap = MotionStatus {
        run_id: task.run_id.clone(),
        role: task.motor.common.role.clone(),
        kind: intent.kind_str().to_string(),
        t_ms: Utc::now().timestamp_millis(),
        state: MotionState::Running,
        vel_rad_s: vel,
        mech_pos_rad,
        reason: None,
    };
    let _ = task.state.motion_status_tx.send(snap);
}

fn broadcast_stopped(task: &ControllerTask, mech_pos_rad: f32, reason: &MotionStopReason) {
    let intent = task.intent_rx.borrow().clone();
    let snap = MotionStatus {
        run_id: task.run_id.clone(),
        role: task.motor.common.role.clone(),
        kind: intent.kind_str().to_string(),
        t_ms: Utc::now().timestamp_millis(),
        state: MotionState::Stopped,
        vel_rad_s: 0.0,
        mech_pos_rad,
        reason: Some(reason.label().to_string()),
    };
    if task.state.motion_status_tx.send(snap).is_err() {
        // No subscribers; harmless. The audit log carries the same
        // information for offline review.
    }

    // Also fire a SafetyEvent on non-operator stops so the dashboard's
    // safety log lights up the same way as for the existing
    // jog_watchdog_stop. We reuse the existing TravelLimitViolation
    // shape only for the band-violation case to keep wire types stable;
    // other reasons emit nothing extra (the audit log + MotionStatus
    // suffice).
    if let MotionStopReason::TravelLimitViolation = reason {
        if let Some(limits) = task
            .state
            .inventory
            .read()
            .expect("inventory poisoned")
            .actuator_by_role(&task.motor.common.role)
            .and_then(|m| m.common.travel_limits.clone())
        {
            let _ =
                task.state
                    .safety_event_tx
                    .send(crate::types::SafetyEvent::TravelLimitViolation {
                        t_ms: Utc::now().timestamp_millis(),
                        role: task.motor.common.role.clone(),
                        attempted_rad: mech_pos_rad,
                        min_rad: limits.min_rad,
                        max_rad: limits.max_rad,
                    });
        }
    }

    // Boot state may have been lost mid-motion (e.g. fault); leave
    // boot_state where it is — the telemetry classifier will re-evaluate
    // on the next tick.
    let _ = boot_state::current(&task.state, &task.motor.common.role);
}

fn audit_start(task: &ControllerTask) {
    let intent = task.intent_rx.borrow().clone();
    task.state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "motion_start".into(),
        target: Some(task.motor.common.role.clone()),
        details: serde_json::json!({
            "run_id": task.run_id,
            "kind": intent.kind_str(),
            "intent": serde_json::to_value(&intent).unwrap_or(serde_json::Value::Null),
        }),
        result: AuditResult::Ok,
    });
}

fn audit_stop(task: &ControllerTask, reason: &MotionStopReason) {
    let result = match reason {
        MotionStopReason::Operator
        | MotionStopReason::ClientGone
        | MotionStopReason::Superseded
        | MotionStopReason::Shutdown => AuditResult::Ok,
        _ => AuditResult::Denied,
    };
    if matches!(result, AuditResult::Denied) {
        warn!(
            role = %task.motor.common.role,
            run_id = %task.run_id,
            reason = reason.label(),
            "motion aborted"
        );
    }
    task.state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "motion_stop".into(),
        target: Some(task.motor.common.role.clone()),
        details: serde_json::json!({
            "run_id": task.run_id,
            "reason": reason.label(),
            "detail": reason.detail(),
        }),
        result,
    });
}
