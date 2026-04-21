//! REST API router.
//!
//! All routes return JSON. Mutating endpoints audit-log their inputs and
//! (when applicable) range-check against the actuator spec.

use axum::{
    routing::{get, post, put},
    Router,
};

use crate::state::SharedState;

mod commission;
mod config_route;
mod control;
mod devices;
mod estop;
mod hardware;
mod health;
mod home;
mod home_all;
mod inventory_route;
mod jog;
mod logs;
mod motion;
mod motors;
mod onboard;
mod params;
mod predefined_home;
mod reminders_route;
mod rename;
mod restore_offset;
mod system;
mod tests;
mod travel;

pub fn router(state: SharedState) -> Router<SharedState> {
    Router::new()
        .route("/config", get(config_route::get_config))
        .route("/health", get(health::get_health))
        .route("/system", get(system::get_system))
        .route("/devices", get(devices::list_devices))
        .route("/hardware/unassigned", get(hardware::list_unassigned))
        .route("/hardware/scan", post(hardware::scan))
        .route(
            "/hardware/onboard/robstride",
            post(onboard::onboard_robstride),
        )
        .route("/motors", get(motors::list_motors))
        .route("/motors/:role", get(motors::get_motor))
        .route("/motors/:role/feedback", get(motors::get_feedback))
        .route("/motors/:role/params", get(params::get_params))
        .route("/motors/:role/params/:name", put(params::put_param))
        .route("/motors/:role/save", post(control::save_to_flash))
        .route("/motors/:role/enable", post(control::enable))
        .route("/motors/:role/stop", post(control::stop))
        .route("/motors/:role/set_zero", post(control::set_zero))
        .route("/motors/:role/commission", post(commission::commission))
        .route(
            "/motors/:role/restore_offset",
            post(restore_offset::restore_offset),
        )
        .route(
            "/motors/:role/travel_limits",
            get(travel::get_travel_limits).put(travel::put_travel_limits),
        )
        .route(
            "/motors/:role/predefined_home",
            put(predefined_home::put_predefined_home),
        )
        .route("/motors/:role/jog", post(jog::jog))
        .route("/motors/:role/motion", get(motion::get_motion))
        .route("/motors/:role/motion/sweep", post(motion::start_sweep))
        .route("/motors/:role/motion/wave", post(motion::start_wave))
        .route(
            "/motors/:role/motion/jog",
            post(motion::start_or_update_jog),
        )
        .route("/motors/:role/motion/stop", post(motion::stop))
        .route("/motors/:role/home", post(home::home))
        .route("/motors/:role/rename", post(rename::rename))
        .route("/motors/:role/assign", post(rename::assign))
        .route("/home_all", post(home_all::home_all))
        .route("/motors/:role/tests/:name", post(tests::run_test))
        .route(
            "/motors/:role/inventory",
            get(inventory_route::get_inventory),
        )
        .route("/motors/:role/verified", put(inventory_route::put_verified))
        .route("/estop", post(estop::estop))
        .route("/logs", get(logs::list).delete(logs::clear))
        .route("/logs/level", get(logs::get_level).put(logs::put_level))
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
