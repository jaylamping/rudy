//! GET /api/devices — full polymorphic inventory (`devices:` from `inventory.yaml`).

use axum::{extract::State, Json};

use crate::inventory::Device;
use crate::state::SharedState;

pub async fn list_devices(State(state): State<SharedState>) -> Json<Vec<Device>> {
    let inv = state.inventory.read().expect("inventory poisoned");
    Json(inv.devices.clone())
}
