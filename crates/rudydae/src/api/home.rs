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

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::audit::{AuditEntry, AuditResult};
use crate::boot_state::{self, BootState};
use crate::can::slow_ramp;
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
        BootState::OffsetChanged {
            stored_rad,
            current_rad,
        } => {
            return Err(err(
                StatusCode::CONFLICT,
                "offset_changed",
                Some(format!(
                    "{role} commissioned_zero_offset disagrees with firmware \
                     (stored={stored_rad:.4}, current={current_rad:.4}); \
                     re-commission or restore_offset before homing"
                )),
            ));
        }
        BootState::AutoHoming { .. } => {
            return Err(err(
                StatusCode::CONFLICT,
                "auto_homing_in_progress",
                Some(format!(
                    "boot orchestrator is already auto-homing {role}; wait for completion"
                )),
            ));
        }
        BootState::HomeFailed { .. } => {
            // Operator-initiated retry: this is the recovery path for a
            // failed auto-home. Allowed to proceed.
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

    // Long-running motion command — delegate to the shared slow-ramp
    // executor so the boot orchestrator (Phase C.5) can drive the same
    // loop without going through this HTTP handler.
    let outcome = slow_ramp::run(state.clone(), motor.clone(), current_pos, target_rad).await;

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
