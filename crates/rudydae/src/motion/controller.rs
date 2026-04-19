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
//!   cadence. The bus_worker's smart re-arm logic (RUN_MODE + cmd_enable
//!   on the first frame, SPD_REF only thereafter) is the reason a
//!   sustained server-side loop produces smooth motion in the first
//!   place — every task respawn would re-trip the re-arm cycle and
//!   reintroduce the jitter we're killing.
//! * Stop discipline collapses to a single point: every exit path of
//!   `run()` issues `cmd_stop` and clears `state.enabled`. Modeled on
//!   the existing exit gate in [`crate::api::home::run_homer`].
//!
//! The controller does NOT own a copy of the bus or talk to wtransport
//! directly; it routes outbound velocity through [`drive_velocity`] (a
//! light wrapper over `RealCanHandle::set_velocity_setpoint` that is a
//! no-op in mock mode) and outbound status through
//! [`crate::state::AppState::motion_status_tx`].

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{watch, Notify};
use tokio::time::interval;
use tracing::{debug, warn};

use crate::audit::{AuditEntry, AuditResult};
use crate::boot_state;
use crate::inventory::Motor;
use crate::motion::intent::{MotionIntent, MotionState, MotionStatus, MotionStopReason};
use crate::motion::preflight::{PreflightChecks, PreflightFailure};
use crate::motion::sweep::{self, SweepState};
use crate::motion::wave::{self, WaveState};
use crate::state::SharedState;

/// Minimum tick interval. Faster than this is pointless on the bus
/// (CAN frame at 1 Mbps is ~100 us, but the firmware aggregates
/// updates) and risks starving the bus_worker.
const MIN_TICK_INTERVAL_MS: u64 = 16;

/// Default tick interval. Matches the configured `poll_interval_ms`
/// when the cadence-bump plan landed; we re-derive instead of import
/// to avoid a tight coupling on the telemetry config.
const DEFAULT_TICK_INTERVAL_MS: u64 = 16;

/// Heartbeat window for [`MotionIntent::Jog`]. If the operator's
/// dead-man stream doesn't refresh within this many milliseconds, the
/// controller stops. Sized larger than one tick so a single dropped
/// packet doesn't cut a finger-held jog.
pub const JOG_HEARTBEAT_TTL_MS: u64 = 250;

/// All inputs the spawned task needs to drive one motion. Built by the
/// registry; the controller takes ownership.
pub struct ControllerTask {
    pub state: SharedState,
    pub motor: Motor,
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

        // Re-run the band check on the *projected* position after one
        // tick at desired_vel. The preflight call above used vel=0, which
        // covers boot/state but not the upcoming motion direction.
        let projected_check = PreflightChecks {
            state: &task.state,
            role: &role,
            vel_rad_s: desired_vel,
            horizon_ms: tick_interval_ms,
        }
        .run();
        let vel = match projected_check {
            Ok(_) => desired_vel,
            Err(e) => {
                break stop_reason_from_preflight(&e);
            }
        };

        // Drive the bus. Errors here are terminal (the bus is sick); the
        // motor will be stopped by the post-loop cleanup. Mock mode is a
        // no-op so unit tests can drive the loop without hardware.
        if let Err(e) = drive_velocity(&task.state, &task.motor, vel).await {
            break MotionStopReason::BusError(e);
        }
        task.state.mark_enabled(&role);

        broadcast_running(&task, pf.feedback.mech_pos_rad, intent, vel);
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
        | PreflightFailure::UnknownMotor
        | PreflightFailure::Absent
        | PreflightFailure::NotVerified => MotionStopReason::BootStateLost,
        PreflightFailure::LimbQuarantined { .. } => MotionStopReason::BootStateLost,
        PreflightFailure::Internal(s) => MotionStopReason::BusError(s.clone()),
    }
}

async fn drive_velocity(state: &SharedState, motor: &Motor, vel: f32) -> Result<(), String> {
    let Some(core) = state.real_can.clone() else {
        return Ok(());
    };
    let motor_for_blocking = motor.clone();
    tokio::task::spawn_blocking(move || core.set_velocity_setpoint(&motor_for_blocking, vel))
        .await
        .map_err(|e| format!("spawn_blocking: {e}"))?
        .map_err(|e| format!("{e:#}"))
}

async fn drive_stop(state: &SharedState, motor: &Motor) -> Result<(), String> {
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
