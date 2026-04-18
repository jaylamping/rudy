//! GET /api/system - host-metrics snapshot for the operator-console dashboard.

use axum::{extract::State, Json};

use crate::state::SharedState;
use crate::types::SystemSnapshot;

pub async fn get_system(State(state): State<SharedState>) -> Json<SystemSnapshot> {
    let snap = {
        let mut poller = state.system.lock().expect("system poller poisoned");
        poller.snapshot(state.cfg.can.mock)
    };
    Json(snap)
}
