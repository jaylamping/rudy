//! PUT /api/motors/:role/predefined_home — boot orchestrator neutral target.
//!
//! Persists `Motor.predefined_home_rad` in `inventory.yaml` via
//! [`inventory::write_atomic`]. The value must lie within the motor's
//! configured [`TravelLimits`] band (inclusive). Motors without a travel
//! band must configure one first (`PUT .../travel_limits`).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use crate::audit::{AuditEntry, AuditResult};
use crate::inventory;
use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

#[derive(Debug, Deserialize)]
pub struct PredefinedHomeBody {
    pub predefined_home_rad: f32,
}

#[derive(Debug, Serialize)]
pub struct PredefinedHomeResp {
    pub ok: bool,
    pub role: String,
    pub predefined_home_rad: f32,
}

fn err(status: StatusCode, error: &str, detail: Option<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
            detail,
            ..Default::default()
        }),
    )
}

pub async fn put_predefined_home(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<PredefinedHomeBody>,
) -> Result<Json<PredefinedHomeResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);

    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            body.predefined_home_rad,
            "lock_held",
        );
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
        ));
    }

    if !body.predefined_home_rad.is_finite() {
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            body.predefined_home_rad,
            "non_finite",
        );
        return Err(err(
            StatusCode::BAD_REQUEST,
            "out_of_range",
            Some("predefined_home_rad must be finite".into()),
        ));
    }

    let (min_rad, max_rad) = {
        let inv = state.inventory.read().expect("inventory poisoned");
        let motor = inv.by_role(&role).ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            )
        })?;
        let Some(limits) = motor.travel_limits.as_ref() else {
            audit(
                &state,
                session.clone(),
                &role,
                AuditResult::Denied,
                body.predefined_home_rad,
                "no_travel_limits",
            );
            return Err(err(
                StatusCode::CONFLICT,
                "no_travel_limits",
                Some(format!(
                    "motor {role} has no travel_limits; set those before predefined_home_rad"
                )),
            ));
        };
        (limits.min_rad, limits.max_rad)
    };

    if body.predefined_home_rad < min_rad || body.predefined_home_rad > max_rad {
        let detail = format!(
            "predefined_home_rad {} outside travel band [{}, {}]",
            body.predefined_home_rad, min_rad, max_rad
        );
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            body.predefined_home_rad,
            "outside_travel_band",
        );
        return Err(err(
            StatusCode::BAD_REQUEST,
            "outside_travel_band",
            Some(detail),
        ));
    }

    let path = state.cfg.paths.inventory.clone();
    let role_for_closure = role.clone();
    let value = body.predefined_home_rad;
    let new_inv = tokio::task::spawn_blocking(move || {
        inventory::write_atomic(&path, |inv| {
            let m = inv.motors.iter_mut().find(|m| m.role == role_for_closure);
            let Some(m) = m else {
                anyhow::bail!("motor {role_for_closure} disappeared from inventory");
            };
            m.predefined_home_rad = Some(value);
            Ok(())
        })
    })
    .await
    .expect("predefined_home write task panicked")
    .map_err(|e| {
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            body.predefined_home_rad,
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
        action: "predefined_home_set".into(),
        target: Some(role.clone()),
        details: serde_json::json!({
            "predefined_home_rad": body.predefined_home_rad,
        }),
        result: AuditResult::Ok,
    });

    Ok(Json(PredefinedHomeResp {
        ok: true,
        role,
        predefined_home_rad: body.predefined_home_rad,
    }))
}

fn audit(
    state: &SharedState,
    session: Option<String>,
    role: &str,
    result: AuditResult,
    predefined_home_rad: f32,
    reason: &str,
) {
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "predefined_home_set".into(),
        target: Some(role.into()),
        details: serde_json::json!({
            "predefined_home_rad": predefined_home_rad,
            "reason": reason,
        }),
        result,
    });
}
