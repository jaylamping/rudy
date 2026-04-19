//! GET /api/motors, GET /api/motors/:role, GET /api/motors/:role/feedback.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use crate::boot_state;
use crate::state::SharedState;
use crate::types::{ApiError, MotorFeedback, MotorSummary};

pub async fn list_motors(State(state): State<SharedState>) -> Json<Vec<MotorSummary>> {
    let latest = state.latest.read().expect("latest poisoned");
    let inv = state.inventory.read().expect("inventory poisoned");
    let out = inv
        .motors
        .iter()
        .map(|m| summary_for(&state, m, latest.get(&m.role).cloned()))
        .collect();
    Json(out)
}

pub async fn get_motor(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<MotorSummary>, (StatusCode, Json<ApiError>)> {
    let inv = state.inventory.read().expect("inventory poisoned");
    let motor = inv.by_role(&role).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "unknown_motor".into(),
                detail: Some(format!("no motor with role={role}")),
                ..Default::default()
            }),
        )
    })?;
    let latest = state
        .latest
        .read()
        .expect("latest poisoned")
        .get(&role)
        .cloned();
    let summary = summary_for(&state, motor, latest);
    Ok(Json(summary))
}

/// Build a `MotorSummary` from inventory + latest + boot state. Pulled
/// out so list / get stay 1:1 (the SPA destructures the same shape from both).
fn summary_for(
    state: &SharedState,
    m: &crate::inventory::Motor,
    latest: Option<MotorFeedback>,
) -> MotorSummary {
    MotorSummary {
        role: m.role.clone(),
        can_bus: m.can_bus.clone(),
        can_id: m.can_id,
        firmware_version: m.firmware_version.clone(),
        verified: m.verified,
        present: m.present,
        travel_limits: m.travel_limits.clone(),
        predefined_home_rad: m.predefined_home_rad,
        latest,
        boot_state: boot_state::current(state, &m.role),
        limb: m.limb.clone(),
        joint_kind: m.joint_kind,
    }
}

pub async fn get_feedback(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<MotorFeedback>, (StatusCode, Json<ApiError>)> {
    state
        .latest
        .read()
        .expect("latest poisoned")
        .get(&role)
        .cloned()
        .map(Json)
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "no_feedback".into(),
                detail: Some(format!("no feedback yet for role={role}")),
                ..Default::default()
            }),
        ))
}
