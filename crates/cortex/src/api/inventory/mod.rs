//! Inventory and discovery routes (devices, hardware, onboard, motor listing, record).

use axum::{
    routing::{delete, get, post, put},
    Router,
};

use crate::state::SharedState;

mod devices;
mod hardware;
mod motor;
mod onboard;
mod record;
mod rename;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/devices", get(devices::list_devices))
        .route("/devices/:role", delete(devices::remove_device))
        .route("/hardware/unassigned", get(hardware::list_unassigned))
        .route("/hardware/scan", post(hardware::scan))
        .route(
            "/hardware/onboard/robstride",
            post(onboard::onboard_robstride),
        )
        .route("/motors", get(motor::list_motors))
        .route("/motors/:role", get(motor::get_motor))
        .route("/motors/:role/feedback", get(motor::get_feedback))
        .route("/motors/:role/inventory", get(record::get_inventory))
        .route("/motors/:role/verified", put(record::put_verified))
        .route("/motors/:role/rename", post(rename::rename))
        .route("/motors/:role/assign", post(rename::assign))
}
