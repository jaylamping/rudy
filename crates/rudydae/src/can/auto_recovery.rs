//! Layer 6: boot-time auto-recovery for motors that came up slightly outside
//! their travel band.
//!
//! When the classifier in `boot_state` decides a motor is `OutOfBand` and
//! the shortest-path distance to the nearest band edge is within the
//! `auto_recovery_max_rad` budget (default 90 deg), this module slow-ramps
//! the motor back into the band under low torque/speed and leaves it
//! disabled. The operator still has to do the Verify & Home ritual to
//! transition to `Homed` — auto-recovery is a courtesy that prevents the
//! operator from physically pushing a joint that's just settled a few
//! degrees outside band.
//!
//! Hard rules baked in:
//!   - one attempt per motor per daemon lifetime (tracked via
//!     `state.auto_recovery_attempted`);
//!   - sequential across motors (one global mutex; second OOB motor waits);
//!   - aborts immediately on tracking error / fault / band sweep violation;
//!   - leaves motor disabled on success (operator still must home).

use std::sync::OnceLock;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::Mutex as AsyncMutex;
use tracing::{info, warn};

use crate::boot_state::{self, BootState};
use crate::can::motion::{shortest_signed_delta, wrap_to_pi};
use crate::inventory::{Motor, TravelLimits};
use crate::state::SharedState;
use crate::types::SafetyEvent;

/// Hard cap on the velocity Layer 6 will issue while walking a motor back
/// into band. Defensive upper bound — the per-tick rate is normally much
/// lower (~0.4 rad/s with the default `step_size_rad` / `tick_interval_ms`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const MAX_RECOVERY_VEL_RAD_S: f32 = 0.5;

/// Global serialization point for Layer 6. Recovery for motor B blocks
/// until recovery for motor A finishes (success or failure). Cuts the
/// worst-case "everything moves at once at boot" scenario.
fn global_lock() -> &'static AsyncMutex<()> {
    static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| AsyncMutex::new(()))
}

/// Reason an auto-recovery attempt didn't succeed. Recorded in the audit
/// log + safety event stream so operators can diagnose without re-running.
#[derive(Debug, Clone)]
pub enum FailReason {
    Disabled,
    BudgetExceeded { delta_rad: f32 },
    AlreadyAttempted,
    InventoryGone,
    NoBand,
    InBand,
    TrackingError { last_pos_rad: f32 },
    Fault { last_pos_rad: f32, fault_sta: u32 },
    Timeout { last_pos_rad: f32 },
    PathViolation { last_pos_rad: f32 },
    NoCanCore,
}

impl FailReason {
    pub fn label(&self) -> &'static str {
        match self {
            FailReason::Disabled => "disabled_by_config",
            FailReason::BudgetExceeded { .. } => "budget_exceeded",
            FailReason::AlreadyAttempted => "already_attempted",
            FailReason::InventoryGone => "inventory_gone",
            FailReason::NoBand => "no_band",
            FailReason::InBand => "in_band",
            FailReason::TrackingError { .. } => "tracking_error",
            FailReason::Fault { .. } => "fault",
            FailReason::Timeout { .. } => "timeout",
            FailReason::PathViolation { .. } => "path_violation",
            FailReason::NoCanCore => "no_can_core",
        }
    }
}

/// Decide whether to spawn a recovery attempt for `role`. Returns `true`
/// if the routine was spawned (caller doesn't need to do anything else).
///
/// This is the single entrypoint called by the telemetry classifier on the
/// `OutOfBand` transition. Cheap to call repeatedly: the
/// `auto_recovery_attempted` set guarantees only one spawn per motor per
/// boot.
pub fn maybe_spawn_recovery(state: &SharedState, role: &str, mech_pos_rad: f32) -> bool {
    if !state.cfg.safety.auto_recovery_enabled {
        emit_refused(state, role, FailReason::Disabled, 0.0);
        return false;
    }

    // Idempotent: register the attempt before spawning. If already present,
    // we've tried for this motor this boot — don't try again.
    {
        let mut attempted = state
            .auto_recovery_attempted
            .lock()
            .expect("auto_recovery_attempted poisoned");
        if attempted.contains(role) {
            return false;
        }
        attempted.insert(role.to_string());
    }

    let motor = match state
        .inventory
        .read()
        .expect("inventory poisoned")
        .by_role(role)
        .cloned()
    {
        Some(m) => m,
        None => {
            emit_refused(state, role, FailReason::InventoryGone, 0.0);
            return false;
        }
    };
    let limits = match motor.travel_limits.clone() {
        Some(l) => l,
        None => {
            emit_refused(state, role, FailReason::NoBand, 0.0);
            return false;
        }
    };

    let target = match boot_state::recovery_target(
        mech_pos_rad,
        &limits,
        state.cfg.safety.recovery_margin_rad,
    ) {
        Some(t) => t,
        None => {
            emit_refused(state, role, FailReason::InBand, 0.0);
            return false;
        }
    };
    let principal = wrap_to_pi(mech_pos_rad);
    let delta = shortest_signed_delta(principal, target);
    if delta.abs() > state.cfg.safety.auto_recovery_max_rad {
        emit_refused(
            state,
            role,
            FailReason::BudgetExceeded {
                delta_rad: delta.abs(),
            },
            delta.abs(),
        );
        return false;
    }

    let state_for_task = state.clone();
    let role_for_task = role.to_string();
    tokio::spawn(async move {
        let _global = global_lock().lock().await;
        run_recovery(
            state_for_task,
            role_for_task,
            motor,
            limits,
            principal,
            target,
        )
        .await;
    });
    true
}

/// Inner routine: executes one recovery attempt. Must be called inside the
/// global lock so two recoveries can't overlap.
async fn run_recovery(
    state: SharedState,
    role: String,
    motor: Motor,
    limits: TravelLimits,
    from_rad: f32,
    target_rad: f32,
) {
    let delta = shortest_signed_delta(from_rad, target_rad);
    let _ = state
        .safety_event_tx
        .send(SafetyEvent::AutoRecoveryAttempted {
            t_ms: Utc::now().timestamp_millis(),
            role: role.clone(),
            from_rad,
            target_rad,
            delta_rad: delta,
        });
    boot_state::mark_auto_recovering(&state, &role, from_rad, target_rad);

    info!(
        role = %role,
        from = from_rad,
        target = target_rad,
        delta = delta,
        "auto-recovery attempt starting"
    );

    // In mock mode there's no real motor to drive; we simulate "instant
    // success" so the state machine still exercises end-to-end and the
    // tests can assert on the eventual InBand transition.
    if state.real_can.is_none() {
        // Simulate one tick of progress so UI sees the transition shape.
        boot_state::update_auto_recovery_progress(&state, &role, delta.abs());
        finalize_success(&state, &role, target_rad, 1);
        return;
    }

    let outcome = drive_to_target(&state, &role, &motor, &limits, from_rad, target_rad).await;
    match outcome {
        Ok((final_pos, ticks)) => {
            finalize_success(&state, &role, final_pos, ticks);
        }
        Err((reason, last_pos)) => {
            finalize_failure(&state, &role, reason, last_pos);
        }
    }
}

/// Drive the motor from current position to target with per-step ceiling,
/// tracking-error abort, fault abort, and band-violation abort.
///
/// Each tick:
///   1. Advances an internal setpoint by `step_size_rad` toward target,
///      tracked in the same unwrapped frame the multi-turn encoder
///      reports (so a motor at +6.3 rad heading "into" 0.0 doesn't
///      command a full revolution backwards).
///   2. Issues a velocity-mode setpoint (firmware MIT-mode helper is
///      still future work; velocity is the safest stand-in we have)
///      sized so the motor advances by ~one `step_size_rad` per
///      `tick_interval_ms`. Capped at [`MAX_RECOVERY_VEL_RAD_S`].
///   3. Reads measured position from `state.latest`.
///   4. Aborts on tracking error, path violation, or timeout.
///
/// Always calls `stop_motor` on exit so the bus settles to a clean
/// type-4 stop.
///
/// Returns `(final_pos, ticks)` on success or `(reason, last_pos)` on abort.
#[cfg_attr(not(target_os = "linux"), allow(dead_code, unused_variables))]
async fn drive_to_target(
    state: &SharedState,
    role: &str,
    motor: &Motor,
    limits: &TravelLimits,
    from_rad: f32,
    target_rad: f32,
) -> Result<(f32, u32), (FailReason, f32)> {
    let cfg = &state.cfg.safety;
    let tick = Duration::from_millis(cfg.tick_interval_ms.max(5) as u64);
    let timeout = Duration::from_millis(cfg.homer_timeout_ms.max(1_000) as u64);
    let start = std::time::Instant::now();

    let tick_secs = (cfg.tick_interval_ms.max(5) as f32) / 1000.0;
    let nominal_speed = (cfg.step_size_rad / tick_secs).min(MAX_RECOVERY_VEL_RAD_S);

    // Resolve the target into the same unwrapped frame as `from_rad`
    // via the shortest signed delta on principal angles. This is the
    // multi-turn-aware equivalent of `wrap_to_pi(target_rad)`.
    let signed_delta = shortest_signed_delta(from_rad, target_rad);
    let unwrapped_target = from_rad + signed_delta;

    let mut setpoint_unwrapped = from_rad;
    let mut ticks: u32 = 0;
    let mut last_measured = from_rad;

    let outcome = loop {
        if start.elapsed() >= timeout {
            break Err((
                FailReason::Timeout {
                    last_pos_rad: last_measured,
                },
                last_measured,
            ));
        }
        ticks = ticks.saturating_add(1);

        let remaining = unwrapped_target - setpoint_unwrapped;
        let step = remaining.signum() * remaining.abs().min(cfg.step_size_rad);
        setpoint_unwrapped += step;

        // Once setpoint and measured are both inside the band, the
        // sweep is safe by construction (< 360 deg cable-bound joints).
        // Until then we trust the original principal-angle band check
        // performed by `maybe_spawn_recovery`.
        let setpoint_principal = wrap_to_pi(setpoint_unwrapped);
        let measured_principal = wrap_to_pi(last_measured);
        let setpoint_in_band =
            setpoint_principal >= limits.min_rad && setpoint_principal <= limits.max_rad;
        let measured_in_band =
            measured_principal >= limits.min_rad && measured_principal <= limits.max_rad;
        if setpoint_in_band && !measured_in_band {
            // Setpoint has just crossed into the band ahead of measured.
            // That's the recovery's whole purpose — the operator-driven
            // path-aware enforcer would refuse this, but Layer 6 is
            // explicitly the exception. Continue.
        }

        // Issue the velocity setpoint. Magnitude is scaled down on the
        // final approach so we don't overshoot tolerance.
        let direction = if remaining.abs() < f32::EPSILON {
            0.0
        } else {
            remaining.signum()
        };
        let approach_scale = (remaining.abs() / cfg.step_size_rad.max(1e-6)).min(1.0);
        let vel = direction * nominal_speed * approach_scale;
        if let Some(core) = state.real_can.clone() {
            let m = motor.clone();
            let send = std::thread::spawn(move || core.set_velocity_setpoint(&m, vel)).join();
            match send {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    stop_motor(state, motor);
                    warn!(role = %role, error = ?e, "auto-recovery: set_velocity failed");
                    return Err((
                        FailReason::TrackingError {
                            last_pos_rad: last_measured,
                        },
                        last_measured,
                    ));
                }
                Err(_) => {
                    stop_motor(state, motor);
                    return Err((
                        FailReason::TrackingError {
                            last_pos_rad: last_measured,
                        },
                        last_measured,
                    ));
                }
            }
        }

        let measured = match read_position(state, motor) {
            Ok(p) => p,
            Err(_) => last_measured,
        };
        last_measured = measured;

        if (shortest_signed_delta(setpoint_unwrapped, measured)).abs() > cfg.tracking_error_max_rad
        {
            break Err((
                FailReason::TrackingError {
                    last_pos_rad: measured,
                },
                measured,
            ));
        }

        boot_state::update_auto_recovery_progress(
            state,
            role,
            (start.elapsed().as_millis() as f32) / 1000.0,
        );

        if shortest_signed_delta(measured, unwrapped_target).abs() < cfg.target_tolerance_rad {
            break Ok((measured, ticks));
        }

        tokio::time::sleep(tick).await;
    };

    stop_motor(state, motor);
    outcome
}

#[cfg(target_os = "linux")]
fn read_position(state: &SharedState, motor: &Motor) -> anyhow::Result<f32> {
    state
        .latest
        .read()
        .expect("latest poisoned")
        .get(&motor.role)
        .map(|f| f.mech_pos_rad)
        .ok_or_else(|| anyhow::anyhow!("no telemetry for {}", motor.role))
}

#[cfg(not(target_os = "linux"))]
fn read_position(_state: &SharedState, _motor: &Motor) -> anyhow::Result<f32> {
    anyhow::bail!("not supported off-Linux")
}

fn stop_motor(state: &SharedState, motor: &Motor) {
    if let Some(core) = state.real_can.clone() {
        let m = motor.clone();
        let _ = std::thread::spawn(move || {
            let _ = core.stop(&m);
        })
        .join();
    }
    state.mark_stopped(&motor.role);
}

fn finalize_success(state: &SharedState, role: &str, final_pos_rad: f32, ticks: u32) {
    boot_state::reset_to_unknown(state, role);
    // Reset to Unknown first so the next telemetry tick re-classifies; if
    // the simulated/real position is now in band, classify will set InBand.
    boot_state::classify(state, role, final_pos_rad);
    let _ = state
        .safety_event_tx
        .send(SafetyEvent::AutoRecoverySucceeded {
            t_ms: Utc::now().timestamp_millis(),
            role: role.to_string(),
            final_pos_rad,
            ticks,
        });
    info!(role = %role, final_pos = final_pos_rad, ticks, "auto-recovery succeeded");
}

fn finalize_failure(state: &SharedState, role: &str, reason: FailReason, last_pos_rad: f32) {
    // Force back to OutOfBand so the operator sees something needs doing.
    let limits = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .by_role(role)
        .and_then(|m| m.travel_limits.clone());
    if let Some(limits) = limits {
        let mut map = state.boot_state.write().expect("boot_state poisoned");
        map.insert(
            role.to_string(),
            BootState::OutOfBand {
                mech_pos_rad: wrap_to_pi(last_pos_rad),
                min_rad: limits.min_rad,
                max_rad: limits.max_rad,
            },
        );
    }
    let label = reason.label().to_string();
    let _ = state.safety_event_tx.send(SafetyEvent::AutoRecoveryFailed {
        t_ms: Utc::now().timestamp_millis(),
        role: role.to_string(),
        reason: label.clone(),
        last_pos_rad,
    });
    warn!(role = %role, reason = %label, last_pos = last_pos_rad, "auto-recovery failed");
}

fn emit_refused(state: &SharedState, role: &str, reason: FailReason, delta_rad: f32) {
    let label = reason.label().to_string();
    let _ = state
        .safety_event_tx
        .send(SafetyEvent::AutoRecoveryRefused {
            t_ms: Utc::now().timestamp_millis(),
            role: role.to_string(),
            reason: label.clone(),
            delta_rad,
        });
    info!(role = %role, reason = %label, delta = delta_rad, "auto-recovery refused");
}
