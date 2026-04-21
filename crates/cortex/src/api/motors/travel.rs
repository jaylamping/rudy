//! GET / PUT /api/motors/:role/travel_limits.
//!
//! `GET` returns the per-motor band currently on disk (or 404 if the motor
//! has no `travel_limits` field — not an error, just "no band configured").
//! `PUT` validates against the hardware outer rail, atomically rewrites
//! `inventory.yaml`, hot-swaps `state.inventory`, and audits the change.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;

use crate::api::error::err;
use crate::audit::{AuditEntry, AuditResult};
use crate::can::travel::validate_band;
use crate::inventory::{self, Device, TravelLimits};
use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

#[derive(Debug, Deserialize)]
pub struct TravelLimitsBody {
    pub min_rad: f32,
    pub max_rad: f32,
}

pub async fn get_travel_limits(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<TravelLimits>, (StatusCode, Json<ApiError>)> {
    let inv = state.inventory.read().expect("inventory poisoned");
    let motor = inv.actuator_by_role(&role).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "unknown_motor",
            Some(format!("no motor with role={role}")),
        )
    })?;
    motor.common.travel_limits.clone().map(Json).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "no_travel_limits",
            Some(format!("motor {role} has no travel_limits configured")),
        )
    })
}

pub async fn put_travel_limits(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<TravelLimitsBody>,
) -> Result<Json<TravelLimits>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);

    // Lock-gate write attempts. First mutator from a fresh session implicitly
    // claims the lock (see `AppState::ensure_control`); a second concurrent
    // session is refused with 423 so two tabs can't silently fight over the
    // same inventory file.
    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        audit_denied(&state, session.as_deref(), &role, "lock_held", &body);
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
        ));
    }

    let model = {
        let inv = state.inventory.read().expect("inventory poisoned");
        let motor = inv.actuator_by_role(&role).ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            )
        })?;
        motor.robstride_model()
    };
    let (hw_lo, hw_hi) = state.spec_for(model).mit_position_rail_rad();

    if let Err(reason) = validate_band(body.min_rad, body.max_rad, hw_lo, hw_hi) {
        audit_denied(&state, session.as_deref(), &role, reason, &body);
        return Err(err(
            StatusCode::BAD_REQUEST,
            "out_of_range",
            Some(reason.to_string()),
        ));
    }

    let limits = TravelLimits {
        min_rad: body.min_rad,
        max_rad: body.max_rad,
        updated_at: Some(Utc::now().to_rfc3339()),
    };

    let path = state.cfg.paths.inventory.clone();
    let role_for_closure = role.clone();
    let limits_for_closure = limits.clone();
    let new_inv = tokio::task::spawn_blocking(move || {
        inventory::write_atomic(&path, |inv| {
            for d in &mut inv.devices {
                if let Device::Actuator(a) = d {
                    if a.common.role == role_for_closure {
                        a.common.travel_limits = Some(limits_for_closure);
                        return Ok(());
                    }
                }
            }
            anyhow::bail!("motor {role_for_closure} disappeared from inventory");
        })
    })
    .await
    .expect("travel_limits write task panicked")
    .map_err(|e| {
        audit_denied(&state, session.as_deref(), &role, "write_failed", &body);
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
        action: "travel_limits_set".into(),
        target: Some(role),
        details: serde_json::json!({
            "min_rad": body.min_rad,
            "max_rad": body.max_rad,
        }),
        result: AuditResult::Ok,
    });

    Ok(Json(limits))
}

fn audit_denied(
    state: &SharedState,
    session: Option<&str>,
    role: &str,
    reason: &str,
    body: &TravelLimitsBody,
) {
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session.map(str::to_string),
        remote: None,
        action: "travel_limits_set".into(),
        target: Some(role.into()),
        details: serde_json::json!({
            "min_rad": body.min_rad,
            "max_rad": body.max_rad,
            "reason": reason,
        }),
        result: AuditResult::Denied,
    });
}
