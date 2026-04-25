//! GET /api/motors/:role/inventory  — full passthrough of the YAML record.
//! PUT /api/motors/:role/verified   — flip the verified flag.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;

use crate::api::error::err;
use crate::audit::{AuditEntry, AuditResult};
use crate::inventory::{self, Device};
use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

/// Full per-motor record (typed scalars + free-form `extra` map). Returned
/// as a JSON object; the SPA renders it in a key/value table on the
/// Inventory tab without needing schema knowledge.
pub async fn get_inventory(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let inv = state.inventory.read().expect("inventory poisoned");
    let motor = inv.actuator_by_role(&role).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "unknown_motor",
            Some(format!("no motor with role={role}")),
        )
    })?;

    // serde_json round-trip of the actuator record (common + family).
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
    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
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
            Some(format!("control lock is held by session {holder}")),
        ));
    }

    {
        let inv = state.inventory.read().expect("inventory poisoned");
        if inv.actuator_by_role(&role).is_none() {
            return Err(err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            ));
        }
    }

    let path = state.cfg.paths.inventory.clone();
    let db_ctx = state.runtime_inventory_persist();
    let role_for_closure = role.clone();
    let new_verified = body.verified;
    let new_inv = tokio::task::spawn_blocking(move || {
        inventory::write_atomic(&path, db_ctx, |inv| {
            for d in &mut inv.devices {
                if let Device::Actuator(a) = d {
                    if a.common.role == role_for_closure {
                        a.common.verified = new_verified;
                        return Ok(());
                    }
                }
            }
            anyhow::bail!("motor disappeared from inventory");
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
