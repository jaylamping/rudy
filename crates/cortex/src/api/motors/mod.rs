//! Per-motor mutating routes (params, control, travel, commission, bench, …).

use axum::{
    routing::{get, post, put},
    Router,
};

use crate::state::SharedState;

mod bench;
mod commission;
mod control;
mod params;
mod predefined_home;
mod restore_offset;
mod travel;

pub fn router() -> Router<SharedState> {
    Router::new()
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
        .route("/motors/:role/tests/:name", post(bench::run_test))
}
