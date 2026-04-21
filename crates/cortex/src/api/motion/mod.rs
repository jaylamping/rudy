//! Motion routes (server-side patterns, jog shim, homing).

use axum::{
    routing::{get, post},
    Router,
};

use crate::state::SharedState;

mod home;
mod home_all;
mod jog;
mod run;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/motors/:role/jog", post(jog::jog))
        .route("/motors/:role/motion", get(run::get_motion))
        .route("/motors/:role/motion/sweep", post(run::start_sweep))
        .route("/motors/:role/motion/wave", post(run::start_wave))
        .route("/motors/:role/motion/jog", post(run::start_or_update_jog))
        .route("/motors/:role/motion/stop", post(run::stop))
        .route("/motors/:role/home", post(home::home))
        .route("/home_all", post(home_all::home_all))
}
