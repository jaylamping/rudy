//! GET /api/motors/:role/inventory  — full passthrough of the YAML record.
//! PUT /api/motors/:role/verified   — flip the verified flag.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;

use crate::audit::{AuditEntry, AuditResult};
use crate::inventory;
use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

fn err(status: StatusCode, error: &str, detail: Option<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
            detail,
        }),
    )
}

/// Full per-motor record (typed scalars + free-form `extra` map). Returned
/// as a JSON object; the SPA renders it in a key/value table on the
/// Inventory tab without needing schema knowledge.
pub async fn get_inventory(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let inv = state.inventory.read().expect("inventory poisoned");
    let motor = inv.by_role(&role).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "unknown_motor",
            Some(format!("no motor with role={role}")),
        )
    })?;

    // serde_json round-trip: this picks up `#[serde(flatten)] extra` so the
    // SPA sees every field the YAML defines, not just the typed ones.
    let value = serde_json::to_value(motor).map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "serialize_failed",
            Some(format!("{e:#}")),
        )
    })?;
    Ok(Json(value))
}

#[derive(Debug, Deserialize)]
pub struct VerifiedBody {
    pub verified: bool,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct VerifiedResp {
    pub ok: bool,
    pub verified: bool,
}

pub async fn put_verified(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<VerifiedBody>,
) -> Result<Json<VerifiedResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);
    if !state.has_control(session.as_deref().unwrap_or("")) {
        state.audit.write(AuditEntry {
            timestamp: Utc::now(),
            session_id: session.clone(),
            remote: None,
            action: "verified_set".into(),
            target: Some(role.clone()),
            details: serde_json::json!({"verified": body.verified, "reason": "lock_held"}),
            result: AuditResult::Denied,
        });
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some("another operator holds the control lock".into()),
        ));
    }

    {
        let inv = state.inventory.read().expect("inventory poisoned");
        if inv.by_role(&role).is_none() {
            return Err(err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            ));
        }
    }

    let path = state.cfg.paths.inventory.clone();
    let role_for_closure = role.clone();
    let new_verified = body.verified;
    let new_inv = tokio::task::spawn_blocking(move || {
        inventory::write_atomic(&path, |inv| {
            let m = inv
                .motors
                .iter_mut()
                .find(|m| m.role == role_for_closure)
                .ok_or_else(|| anyhow::anyhow!("motor disappeared from inventory"))?;
            m.verified = new_verified;
            Ok(())
        })
    })
    .await
    .expect("verified write task panicked")
    .map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "inventory_write_failed",
            Some(format!("{e:#}")),
        )
    })?;

    *state.inventory.write().expect("inventory poisoned") = new_inv;

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "verified_set".into(),
        target: Some(role),
        details: serde_json::json!({
            "verified": body.verified,
            "note": body.note,
        }),
        result: AuditResult::Ok,
    });

    Ok(Json(VerifiedResp {
        ok: true,
        verified: body.verified,
    }))
}
