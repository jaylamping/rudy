//! GET /api/motors/:role/params, PUT /api/motors/:role/params/:name.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;

use crate::audit::{AuditEntry, AuditResult};
use crate::state::SharedState;
use crate::types::{ApiError, ParamSnapshot, ParamWrite};

pub async fn get_params(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<ParamSnapshot>, (StatusCode, Json<ApiError>)> {
    state
        .params
        .read()
        .expect("params poisoned")
        .get(&role)
        .cloned()
        .map(Json)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "unknown_motor".into(),
                    detail: Some(format!("no motor with role={role}")),
                }),
            )
        })
}

pub async fn put_param(
    State(state): State<SharedState>,
    Path((role, name)): Path<(String, String)>,
    Json(body): Json<ParamWrite>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let err = |status, error: &str, detail: Option<String>| {
        (
            status,
            Json(ApiError {
                error: error.into(),
                detail,
            }),
        )
    };

    let motor = state.inventory.by_role(&role).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "unknown_motor",
            Some(format!("no motor with role={role}")),
        )
    })?;

    let desc = state
        .spec
        .firmware_limits
        .get(&name)
        .or_else(|| state.spec.observables.get(&name))
        .cloned()
        .ok_or_else(|| {
            err(
                StatusCode::BAD_REQUEST,
                "unknown_param",
                Some(format!("no parameter with name={name}")),
            )
        })?;

    // Range-check floats against hardware_range.
    if let (Some([lo, hi]), Some(v)) = (desc.hardware_range, body.value.as_f64()) {
        if (v as f32) < lo || (v as f32) > hi {
            let entry = AuditEntry {
                timestamp: Utc::now(),
                session_id: None,
                remote: None,
                action: "param_write".into(),
                target: Some(format!("{role}.{name}")),
                details: serde_json::json!({ "value": body.value, "range": [lo, hi] }),
                result: AuditResult::Denied,
            };
            state.audit.write(entry);
            return Err(err(
                StatusCode::BAD_REQUEST,
                "out_of_range",
                Some(format!("{v} not in [{lo}, {hi}]")),
            ));
        }
    }

    // Mutate the in-memory snapshot. Real CAN wiring will replace this with a
    // call to driver::rs03::session::write_param_*.
    {
        let mut params = state.params.write().expect("params poisoned");
        if let Some(snap) = params.get_mut(&role) {
            if let Some(pv) = snap.values.get_mut(&name) {
                pv.value = body.value.clone();
            }
        }
    }

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: if body.save_after {
            "param_write_and_save"
        } else {
            "param_write"
        }
        .into(),
        target: Some(format!("{role}.{name}")),
        details: serde_json::json!({
            "can_id": motor.can_id,
            "index": format!("0x{:04X}", desc.index),
            "value": body.value,
        }),
        result: AuditResult::Ok,
    });

    Ok(Json(serde_json::json!({
        "ok": true,
        "saved": body.save_after,
        "role": role,
        "name": name,
        "value": body.value,
    })))
}
