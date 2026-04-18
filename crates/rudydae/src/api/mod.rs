//! REST API router.
//!
//! All routes return JSON. Mutating endpoints audit-log their inputs and
//! (when applicable) range-check against the actuator spec.

use axum::{
    routing::{get, post, put},
    Router,
};

use crate::state::SharedState;

mod config_route;
mod control;
mod motors;
mod params;
mod reminders_route;
mod system;

pub fn router(state: SharedState) -> Router<SharedState> {
    Router::new()
        .route("/config", get(config_route::get_config))
        .route("/system", get(system::get_system))
        .route("/motors", get(motors::list_motors))
        .route("/motors/:role", get(motors::get_motor))
        .route("/motors/:role/feedback", get(motors::get_feedback))
        .route("/motors/:role/params", get(params::get_params))
        .route("/motors/:role/params/:name", put(params::put_param))
        .route("/motors/:role/save", post(control::save_to_flash))
        .route("/motors/:role/enable", post(control::enable))
        .route("/motors/:role/stop", post(control::stop))
        .route("/motors/:role/set_zero", post(control::set_zero))
        .route(
            "/reminders",
            get(reminders_route::list).post(reminders_route::create),
        )
        .route(
            "/reminders/:id",
            put(reminders_route::update).delete(reminders_route::delete_one),
        )
        .with_state(state)
}
