//! GET /api/runtime - read-only ADR-0008 runtime FSM snapshot.

use axum::{extract::State, Json};

use crate::state::SharedState;
use crate::types::RuntimeStatus;

pub async fn get_runtime(State(state): State<SharedState>) -> Json<RuntimeStatus> {
    Json(state.runtime_status())
}
