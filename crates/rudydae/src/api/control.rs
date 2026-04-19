//! POST /api/motors/:role/{enable,stop,save,set_zero}.
//!
//! Note on persistence: `save` and `set_zero` look like a pair but they
//! are not. `save` issues a type-22 SaveParams that flushes every
//! RAM-resident parameter to firmware flash. `set_zero` issues a type-6
//! that updates `add_offset` in RAM only and is therefore RAM-only by
//! design — even an immediately-following `save` can race with the
//! firmware's internal flush bookkeeping in ways that have surprised
//! operators in the past. The supported flash-persistent zeroing path is
//! the dedicated `POST /api/motors/:role/commission` endpoint, which
//! sequences type-6 + type-22 + a readback of `add_offset` and records
//! the result in `inventory.yaml`.

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

/// RAM-only set-mechanical-zero (firmware type-6).
///
/// **This endpoint does NOT persist the new zero across power cycles.** It
/// issues a single type-6 frame; the firmware updates `add_offset` (param
/// 0x702B) in RAM but never writes flash. The new zero survives until the
/// motor loses power, at which point the previously-saved `add_offset`
/// (or 0.0 if it was never saved) takes effect again. See ADR-0002 §
/// "type-6 vs type-22" for the wire-protocol detail.
///
/// For a flash-persistent zero — and to record the offset in
/// `inventory.yaml` so the boot orchestrator can verify it on every boot —
/// use `POST /api/motors/:role/commission` instead. That endpoint runs the
/// type-6 + type-22 SaveParams sequence and reads back `add_offset` to
/// confirm the firmware accepted the change.
///
/// This endpoint is retained as a diagnostic tool (an operator may want to
/// briefly re-zero a motor mid-session for a measurement and not commit
/// to it). The audit log records every call with `persisted: false` to
/// make the distinction visible after the fact.
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
            audit_set_zero(&state, &role, AuditResult::Denied);
            can_err("set_zero", &role, &e)
        })?;
    }

    // A re-zero invalidates the prior home attestation: every position
    // the daemon has seen is now measured against a different origin.
    // Reset BootState to Unknown so the next telemetry tick re-classifies
    // and the operator must explicitly re-home before enable will work.
    boot_state::reset_to_unknown(&state, &role);

    audit_set_zero(&state, &role, AuditResult::Ok);
    Ok(Json(serde_json::json!({
        "ok": true,
        "role": role,
        "persisted": false,
    })))
}

/// Audit helper for `set_zero` that always records `persisted: false` so
/// the distinction between RAM-only `set_zero` and flash-persistent
/// `commission` is unambiguous in the audit trail. Operators reviewing
/// the log after a "wait, did this survive the reboot?" question can grep
/// the JSONL for `"persisted":false` to find every RAM-only zero.
fn audit_set_zero(state: &SharedState, role: &str, result: AuditResult) {
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "set_zero".into(),
        target: Some(role.into()),
        details: serde_json::json!({ "persisted": false }),
        result,
    });
}
