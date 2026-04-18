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
mod estop;
mod inventory_route;
mod jog;
mod lock;
mod motors;
mod params;
mod reminders_route;
mod system;
mod tests;
mod travel;

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
            "/motors/:role/travel_limits",
            get(travel::get_travel_limits).put(travel::put_travel_limits),
        )
        .route("/motors/:role/jog", post(jog::jog))
        .route("/motors/:role/tests/:name", post(tests::run_test))
        .route(
            "/motors/:role/inventory",
            get(inventory_route::get_inventory),
        )
        .route("/motors/:role/verified", put(inventory_route::put_verified))
        .route("/estop", post(estop::estop))
        .route(
            "/lock",
            get(lock::get_lock)
                .post(lock::acquire_lock)
                .delete(lock::release_lock),
        )
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
