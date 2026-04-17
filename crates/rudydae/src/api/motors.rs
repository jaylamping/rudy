//! GET /api/motors, GET /api/motors/:role, GET /api/motors/:role/feedback.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use crate::state::SharedState;
use crate::types::{ApiError, MotorFeedback, MotorSummary};

pub async fn list_motors(State(state): State<SharedState>) -> Json<Vec<MotorSummary>> {
    let latest = state.latest.read().expect("latest poisoned");
    let out = state
        .inventory
        .motors
        .iter()
        .map(|m| MotorSummary {
            role: m.role.clone(),
            can_bus: m.can_bus.clone(),
            can_id: m.can_id,
            firmware_version: m.firmware_version.clone(),
            verified: m.verified,
            latest: latest.get(&m.role).cloned(),
        })
        .collect();
    Json(out)
}

pub async fn get_motor(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<MotorSummary>, (StatusCode, Json<ApiError>)> {
    let motor = state.inventory.by_role(&role).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "unknown_motor".into(),
                detail: Some(format!("no motor with role={role}")),
            }),
        )
    })?;
    let latest = state
        .latest
        .read()
        .expect("latest poisoned")
        .get(&role)
        .cloned();
    Ok(Json(MotorSummary {
        role: motor.role.clone(),
        can_bus: motor.can_bus.clone(),
        can_id: motor.can_id,
        firmware_version: motor.firmware_version.clone(),
        verified: motor.verified,
        latest,
    }))
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
            }),
        ))
}
