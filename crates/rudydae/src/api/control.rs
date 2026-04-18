//! POST /api/motors/:role/{enable,stop,save,set_zero}.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;

use crate::audit::{AuditEntry, AuditResult};
use crate::boot_state::{self, BootState};
use crate::can::travel::{enforce_position_with_path, BandCheck};
use crate::state::SharedState;
use crate::types::ApiError;

fn err(status: StatusCode, error: &str, detail: Option<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
            detail,
        }),
    )
}

fn audit(state: &SharedState, action: &str, role: &str, result: AuditResult) {
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: action.into(),
        target: Some(role.into()),
        details: serde_json::Value::Null,
        result,
    });
}

fn can_err(action: &str, role: &str, error: &anyhow::Error) -> (StatusCode, Json<ApiError>) {
    err(
        StatusCode::BAD_GATEWAY,
        "can_command_failed",
        Some(format!("{action} failed for {role}: {error:#}")),
    )
}

/// Resolve `role` against the inventory and reject if the motor is marked
/// `present: false`. Used to fail fast on commands aimed at placeholder /
/// unplugged motors before they queue CAN frames that nothing will ACK
/// (which on a peer-less bus saturates the SocketCAN txqueue and makes
/// every subsequent send return ENOBUFS).
fn require_present(
    state: &SharedState,
    action: &str,
    role: &str,
) -> Result<crate::inventory::Motor, (StatusCode, Json<ApiError>)> {
    let motor = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .by_role(role)
        .cloned()
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            )
        })?;

    if !motor.present {
        audit(state, action, role, AuditResult::Denied);
        return Err(err(
            StatusCode::CONFLICT,
            "motor_absent",
            Some(format!(
                "inventory entry for {role} has present=false; nothing to talk to on the bus"
            )),
        ));
    }

    Ok(motor)
}

pub async fn enable(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let motor = require_present(&state, "enable", &role)?;

    if state.cfg.safety.require_verified && !motor.verified {
        audit(&state, "enable", &role, AuditResult::Denied);
        return Err(err(
            StatusCode::FORBIDDEN,
            "not_verified",
            Some(format!(
                "inventory entry for {role} has verified=false; commission before enabling"
            )),
        ));
    }

    // Check A (the inviolable physics rule): if travel_limits is set the
    // motor must currently be inside the band. Fires regardless of
    // BootState — even if the operator hand-pushed state to Homed, even
    // if telemetry is stale, if the cached position is outside the band
    // we refuse. Check B catches the operational discipline gap when
    // Check A passes.
    if motor.travel_limits.is_some() {
        let cached = state
            .latest
            .read()
            .expect("latest poisoned")
            .get(&role)
            .map(|f| f.mech_pos_rad);
        if let Some(pos) = cached {
            let check = enforce_position_with_path(&state, &role, pos, pos).map_err(|e| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    Some(format!("{e:#}")),
                )
            })?;
            if let BandCheck::OutOfBand {
                min_rad,
                max_rad,
                attempted_rad,
            }
            | BandCheck::PathViolation {
                min_rad,
                max_rad,
                current_rad: attempted_rad,
                ..
            } = check
            {
                audit(&state, "enable", &role, AuditResult::Denied);
                return Err(err(
                    StatusCode::CONFLICT,
                    "out_of_band",
                    Some(format!(
                        "{role} at {attempted_rad:.3} rad outside [{min_rad:.3}, {max_rad:.3}]"
                    )),
                ));
            }
        }
    }

    // Check B (operational discipline): operator must have explicitly
    // homed since the last power-cycle.
    let bs = boot_state::current(&state, &role);
    if !bs.permits_enable() {
        let (code, detail) = match bs {
            BootState::Unknown => (
                "not_ready",
                format!("no telemetry yet for {role}; classify before enabling"),
            ),
            BootState::OutOfBand { .. } => (
                "out_of_band",
                format!("{role} reported outside band; manual recovery required"),
            ),
            BootState::AutoRecovering { .. } => (
                "auto_recovery_in_progress",
                format!("{role} is being driven by auto-recovery"),
            ),
            BootState::InBand => (
                "not_homed",
                format!("POST /motors/{role}/home first to verify position"),
            ),
            BootState::Homed => unreachable!(),
        };
        audit(&state, "enable", &role, AuditResult::Denied);
        return Err(err(StatusCode::CONFLICT, code, Some(detail)));
    }

    if let Some(core) = state.real_can.clone() {
        tokio::task::spawn_blocking({
            let motor = motor.clone();
            move || core.enable(&motor)
        })
        .await
        .expect("enable task panicked")
        .map_err(|e| {
            audit(&state, "enable", &role, AuditResult::Denied);
            can_err("enable", &role, &e)
        })?;
    }

    // Bookkeeping for the rename / assign gates. See `AppState::enabled`
    // for the "tracks intent, not wire state" caveat.
    state.mark_enabled(&role);

    audit(&state, "enable", &role, AuditResult::Ok);
    Ok(Json(serde_json::json!({ "ok": true, "role": role })))
}

pub async fn stop(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let motor = require_present(&state, "stop", &role)?;

    if let Some(core) = state.real_can.clone() {
        tokio::task::spawn_blocking({
            let motor = motor.clone();
            move || core.stop(&motor)
        })
        .await
        .expect("stop task panicked")
        .map_err(|e| {
            audit(&state, "stop", &role, AuditResult::Denied);
            can_err("stop", &role, &e)
        })?;
    }

    state.mark_stopped(&role);

    audit(&state, "stop", &role, AuditResult::Ok);
    Ok(Json(serde_json::json!({ "ok": true, "role": role })))
}

pub async fn save_to_flash(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let motor = require_present(&state, "save_to_flash", &role)?;

    if let Some(core) = state.real_can.clone() {
        tokio::task::spawn_blocking({
            let motor = motor.clone();
            move || core.save_to_flash(&motor)
        })
        .await
        .expect("save_to_flash task panicked")
        .map_err(|e| {
            audit(&state, "save_to_flash", &role, AuditResult::Denied);
            can_err("save_to_flash", &role, &e)
        })?;
    }

    audit(&state, "save_to_flash", &role, AuditResult::Ok);
    Ok(Json(
        serde_json::json!({ "ok": true, "role": role, "saved": true }),
    ))
}

pub async fn set_zero(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let motor = require_present(&state, "set_zero", &role)?;

    if let Some(core) = state.real_can.clone() {
        tokio::task::spawn_blocking({
            let motor = motor.clone();
            move || core.set_zero(&motor)
        })
        .await
        .expect("set_zero task panicked")
        .map_err(|e| {
            audit(&state, "set_zero", &role, AuditResult::Denied);
            can_err("set_zero", &role, &e)
        })?;
    }

    // A re-zero invalidates the prior home attestation: every position
    // the daemon has seen is now measured against a different origin.
    // Reset BootState to Unknown so the next telemetry tick re-classifies
    // and the operator must explicitly re-home before enable will work.
    boot_state::reset_to_unknown(&state, &role);

    audit(&state, "set_zero", &role, AuditResult::Ok);
    Ok(Json(serde_json::json!({ "ok": true, "role": role })))
}
