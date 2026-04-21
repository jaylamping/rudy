//! PUT /api/motors/:role/homing_speed — per-actuator home-ramp speed override.
//!
//! Persists `homing_speed_rad_s` in `inventory.yaml` (`None` clears the override).
//! Values must lie in **[1°/s, 100°/s]** (inclusive), matching the homer cap
//! [`crate::can::home_ramp::MAX_HOMER_VEL_RAD_S`].

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use crate::api::error::err;
use crate::audit::{AuditEntry, AuditResult};
use crate::can::home_ramp::MAX_HOMER_VEL_RAD_S;
use crate::inventory;
use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

const MIN_HOMING_SPEED_RAD_S: f32 = 1.0f32.to_radians();

#[derive(Debug, Deserialize)]
pub struct HomingSpeedBody {
    pub homing_speed_rad_s: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct HomingSpeedResp {
    pub ok: bool,
    pub role: String,
    pub homing_speed_rad_s: Option<f32>,
}

pub async fn put_homing_speed(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<HomingSpeedBody>,
) -> Result<Json<HomingSpeedResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);

    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            body.homing_speed_rad_s,
            "lock_held",
        );
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
        ));
    }

    if let Some(v) = body.homing_speed_rad_s {
        if !v.is_finite() {
            audit(
                &state,
                session.clone(),
                &role,
                AuditResult::Denied,
                Some(v),
                "non_finite",
            );
            return Err(err(
                StatusCode::BAD_REQUEST,
                "out_of_range",
                Some("homing_speed_rad_s must be finite".into()),
            ));
        }
        if !(MIN_HOMING_SPEED_RAD_S..=MAX_HOMER_VEL_RAD_S).contains(&v) {
            audit(
                &state,
                session.clone(),
                &role,
                AuditResult::Denied,
                Some(v),
                "out_of_range",
            );
            return Err(err(
                StatusCode::BAD_REQUEST,
                "out_of_range",
                Some(format!(
                    "homing_speed_rad_s must be between {} and {} rad/s",
                    MIN_HOMING_SPEED_RAD_S, MAX_HOMER_VEL_RAD_S
                )),
            ));
        }
    }

    let path = state.cfg.paths.inventory.clone();
    let role_for_closure = role.clone();
    let value = body.homing_speed_rad_s;
    let new_inv = tokio::task::spawn_blocking(move || {
        inventory::write_atomic(&path, |inv| {
            for d in &mut inv.devices {
                if let inventory::Device::Actuator(a) = d {
                    if a.common.role == role_for_closure {
                        a.common.homing_speed_rad_s = value;
                        return Ok(());
                    }
                }
            }
            anyhow::bail!("motor {role_for_closure} disappeared from inventory");
        })
    })
    .await
    .expect("homing_speed write task panicked")
    .map_err(|e| {
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            body.homing_speed_rad_s,
            "inventory_write_failed",
        );
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
        action: "homing_speed_set".into(),
        target: Some(role.clone()),
        details: serde_json::json!({
            "homing_speed_rad_s": body.homing_speed_rad_s,
        }),
        result: AuditResult::Ok,
    });

    Ok(Json(HomingSpeedResp {
        ok: true,
        role,
        homing_speed_rad_s: body.homing_speed_rad_s,
    }))
}

fn audit(
    state: &SharedState,
    session: Option<String>,
    role: &str,
    result: AuditResult,
    homing_speed_rad_s: Option<f32>,
    reason: &str,
) {
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "homing_speed_set".into(),
        target: Some(role.into()),
        details: serde_json::json!({
            "homing_speed_rad_s": homing_speed_rad_s,
            "reason": reason,
        }),
        result,
    });
}
