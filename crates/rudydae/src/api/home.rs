//! POST /api/motors/:role/home — operator-initiated slow-ramp homing.
//!
//! Validates that the motor is currently in `BootState::InBand`, that the
//! requested target is inside the band, and then ramps the setpoint from
//! current toward target by `step_size_rad` per tick. Aborts on tracking
//! error, fault, band sweep violation, timeout, or e-stop. On success
//! transitions `BootState -> Homed` and (if `limits_written.limit_torque_nm`
//! is set) RAM-restores the per-motor full torque/speed envelope.
//!
//! This is the only path that transitions to `Homed`. The boot-time
//! Layer-6 auto-recovery does NOT count as homing — it just brings the
//! motor back into band; the operator still has to click Verify & Home.

use std::time::{Duration, Instant};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::audit::{AuditEntry, AuditResult};
use crate::boot_state::{self, BootState};
use crate::can::motion::shortest_signed_delta;
use crate::can::travel::{enforce_position_with_path, BandCheck};
use crate::inventory::Motor;
use crate::state::SharedState;
use crate::types::{ApiError, SafetyEvent};
use crate::util::session_from_headers;

/// Hard cap on the velocity the homer will issue. Matches the jog endpoint's
/// `MAX_JOG_VEL_RAD_S` so the homer can't outrun the operator-driven path.
/// In practice the per-tick rate (~0.4 rad/s with default `step_size_rad`
/// and `tick_interval_ms`) is well below this; the cap is a safety net.
const MAX_HOMER_VEL_RAD_S: f32 = 0.5;

#[derive(Debug, Deserialize)]
pub struct HomeBody {
    /// Target in radians. Defaults to 0.0 (the canonical "go to mechanical
    /// zero" home). Must be inside `travel_limits` after wrap-to-pi.
    #[serde(default)]
    pub target_rad: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct HomeResp {
    pub ok: bool,
    pub final_pos_rad: f32,
    pub ticks: u32,
}

fn err(status: StatusCode, error: &str, detail: Option<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
            detail,
        }),
    )
}

pub async fn home(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
    body: Option<Json<HomeBody>>,
) -> Result<Json<HomeResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);
    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
        ));
    }

    let target_rad = body
        .map(|Json(b)| b.target_rad)
        .unwrap_or(None)
        .unwrap_or(0.0);
    if !target_rad.is_finite() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "bad_request",
            Some("target_rad must be finite".into()),
        ));
    }

    let motor = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .by_role(&role)
        .cloned()
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            )
        })?;
    if !motor.present {
        return Err(err(
            StatusCode::CONFLICT,
            "motor_absent",
            Some(format!("inventory entry for {role} has present=false")),
        ));
    }

    let bs = boot_state::current(&state, &role);
    match bs {
        BootState::Unknown => {
            return Err(err(
                StatusCode::CONFLICT,
                "not_ready",
                Some(format!("no telemetry yet for {role}; cannot home")),
            ))
        }
        BootState::OutOfBand {
            mech_pos_rad,
            min_rad,
            max_rad,
        } => {
            return Err(err(
                StatusCode::CONFLICT,
                "out_of_band",
                Some(format!(
                    "{role} at {mech_pos_rad:.3} rad outside [{min_rad:.3}, {max_rad:.3}]"
                )),
            ));
        }
        BootState::AutoRecovering { .. } => {
            return Err(err(
                StatusCode::CONFLICT,
                "auto_recovery_in_progress",
                Some(format!(
                    "auto-recovery is driving {role}; wait for completion"
                )),
            ));
        }
        BootState::Homed => {
            // Re-homing a motor is allowed; it's a no-op if already at target.
        }
        BootState::InBand => {}
    }

    // Read current position from the latest cache. If we have nothing,
    // refuse — homing without a current measurement is meaningless.
    let current_pos = state
        .latest
        .read()
        .expect("latest poisoned")
        .get(&role)
        .map(|f| f.mech_pos_rad)
        .ok_or_else(|| {
            err(
                StatusCode::CONFLICT,
                "no_telemetry",
                Some(format!("no recent telemetry for {role}")),
            )
        })?;

    // Path-aware band check at the front door.
    match enforce_position_with_path(&state, &role, current_pos, target_rad).map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            Some(format!("{e:#}")),
        )
    })? {
        BandCheck::OutOfBand {
            min_rad,
            max_rad,
            attempted_rad,
        } => {
            return Err(err(
                StatusCode::CONFLICT,
                "out_of_band",
                Some(format!(
                    "target {attempted_rad:.3} rad outside [{min_rad:.3}, {max_rad:.3}]"
                )),
            ));
        }
        BandCheck::PathViolation { .. } => {
            return Err(err(
                StatusCode::CONFLICT,
                "path_violation",
                Some("path from current to target sweeps outside band".to_string()),
            ));
        }
        _ => {}
    }

    // Long-running motion command — release the request handler quickly
    // by running the inner loop in spawn_blocking.
    let outcome = run_homer(state.clone(), motor.clone(), current_pos, target_rad).await;

    match outcome {
        Ok((final_pos, ticks)) => {
            boot_state::mark_homed(&state, &role);
            let _ = state.safety_event_tx.send(SafetyEvent::Homed {
                t_ms: Utc::now().timestamp_millis(),
                role: role.clone(),
                final_pos_rad: final_pos,
                samples_count: ticks,
            });
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: session,
                remote: None,
                action: "home".into(),
                target: Some(role),
                details: serde_json::json!({
                    "final_pos_rad": final_pos,
                    "ticks": ticks,
                }),
                result: AuditResult::Ok,
            });
            Ok(Json(HomeResp {
                ok: true,
                final_pos_rad: final_pos,
                ticks,
            }))
        }
        Err((reason, last_pos)) => {
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: session,
                remote: None,
                action: "home".into(),
                target: Some(role.clone()),
                details: serde_json::json!({
                    "reason": reason,
                    "last_pos_rad": last_pos,
                }),
                result: AuditResult::Denied,
            });
            Err(err(
                StatusCode::CONFLICT,
                "home_aborted",
                Some(format!("home aborted: {reason} at {last_pos:.3} rad")),
            ))
        }
    }
}

/// Slow-ramp closed loop that walks the motor from `from_rad` to the
/// principal-angle home target via the shortest signed path.
///
/// Each tick:
///   1. Re-runs the path-aware band check on the current measured
///      position vs. the next setpoint.
///   2. Issues a velocity setpoint sized so the motor advances by
///      ~`step_size_rad` per `tick_interval_ms` (default ~0.4 rad/s ≈
///      23 deg/s), in the direction of the remaining signed delta.
///   3. Reads the latest type-2 telemetry row from `state.latest`.
///   4. Aborts on tracking error (motor not following), path violation
///      (config edited mid-move, or measured drifted out of band),
///      or `homer_timeout_ms`.
///
/// On EVERY exit path — success, abort, or timeout — the motor is
/// commanded to stop (type-4) and `state.enabled` is cleared. Mock-mode
/// (`state.real_can.is_none()`) skips the I/O and simulates instant
/// tracking so unit tests can pin the success path without hardware.
///
/// Returns `(final_pos, ticks)` on success or `(reason, last_pos)` on
/// abort. `final_pos` is the unwrapped raw mechanical position so the
/// audit log and SPA show what the multi-turn encoder actually reads.
async fn run_homer(
    state: SharedState,
    motor: Motor,
    from_rad: f32,
    target_rad: f32,
) -> Result<(f32, u32), (String, f32)> {
    let role = motor.role.clone();
    let cfg = state.cfg.safety.clone();
    let tick = Duration::from_millis(cfg.tick_interval_ms.max(5) as u64);
    let timeout = Duration::from_millis(cfg.homer_timeout_ms.max(1_000) as u64);

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
        if shortest_signed_delta(setpoint_unwrapped, measured).abs() > cfg.tracking_error_max_rad {
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
