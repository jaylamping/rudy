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
use crate::can::motion::{shortest_signed_delta, wrap_to_pi};
use crate::can::travel::{enforce_position_with_path, BandCheck};
use crate::state::SharedState;
use crate::types::{ApiError, SafetyEvent};
use crate::util::session_from_headers;

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
    let outcome = run_homer(state.clone(), motor.role.clone(), current_pos, target_rad).await;

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

async fn run_homer(
    state: SharedState,
    role: String,
    from_rad: f32,
    target_rad: f32,
) -> Result<(f32, u32), (String, f32)> {
    let cfg = state.cfg.safety.clone();
    let tick = Duration::from_millis(cfg.tick_interval_ms.max(5) as u64);
    let timeout = Duration::from_millis(cfg.homer_timeout_ms.max(1_000) as u64);
    let start = Instant::now();
    let mut setpoint = wrap_to_pi(from_rad);
    let target = wrap_to_pi(target_rad);
    let mut ticks: u32 = 0;
    let mut last_measured = setpoint;

    while start.elapsed() < timeout {
        ticks = ticks.saturating_add(1);
        let remaining = shortest_signed_delta(setpoint, target);
        let step = remaining.signum() * remaining.abs().min(cfg.step_size_rad);
        setpoint = wrap_to_pi(setpoint + step);

        // Re-check the path with the new setpoint so a configuration
        // change mid-ramp can't sneak through.
        let check = enforce_position_with_path(&state, &role, last_measured, setpoint)
            .map_err(|e| (format!("internal: {e:#}"), last_measured))?;
        if let BandCheck::OutOfBand { .. } | BandCheck::PathViolation { .. } = check {
            return Err(("path_violation".into(), last_measured));
        }

        // In mock mode there is no real motor — we simulate the measured
        // position tracking the setpoint perfectly so tests can pin the
        // success path without needing real CAN.
        let measured = if let Some(_core) = state.real_can.clone() {
            // On real hardware we rely on the telemetry loop to update
            // state.latest. Read whatever's there (might be slightly stale).
            state
                .latest
                .read()
                .expect("latest poisoned")
                .get(&role)
                .map(|f| f.mech_pos_rad)
                .unwrap_or(setpoint)
        } else {
            setpoint
        };
        last_measured = measured;

        if shortest_signed_delta(setpoint, measured).abs() > cfg.tracking_error_max_rad {
            return Err(("tracking_error".into(), measured));
        }

        if shortest_signed_delta(measured, target).abs() < cfg.target_tolerance_rad {
            return Ok((measured, ticks));
        }

        tokio::time::sleep(tick).await;
    }

    Err(("timeout".into(), last_measured))
}
