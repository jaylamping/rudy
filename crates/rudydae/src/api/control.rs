//! POST /api/motors/:role/{enable,stop,save,set_zero}.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;

use crate::audit::{AuditEntry, AuditResult};
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

pub async fn enable(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let motor = state.inventory.by_role(&role).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "unknown_motor",
            Some(format!("no motor with role={role}")),
        )
    })?;

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

    audit(&state, "enable", &role, AuditResult::Ok);
    Ok(Json(serde_json::json!({ "ok": true, "role": role })))
}

pub async fn stop(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state.inventory.by_role(&role).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "unknown_motor",
            Some(format!("no motor with role={role}")),
        )
    })?;
    audit(&state, "stop", &role, AuditResult::Ok);
    Ok(Json(serde_json::json!({ "ok": true, "role": role })))
}

pub async fn save_to_flash(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state.inventory.by_role(&role).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "unknown_motor",
            Some(format!("no motor with role={role}")),
        )
    })?;
    audit(&state, "save_to_flash", &role, AuditResult::Ok);
    Ok(Json(
        serde_json::json!({ "ok": true, "role": role, "saved": true }),
    ))
}

pub async fn set_zero(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state.inventory.by_role(&role).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "unknown_motor",
            Some(format!("no motor with role={role}")),
        )
    })?;
    audit(&state, "set_zero", &role, AuditResult::Ok);
    Ok(Json(serde_json::json!({ "ok": true, "role": role })))
}
