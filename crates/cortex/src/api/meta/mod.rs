//! Meta routes: config, health, system, logs.

use axum::{routing::get, Router};

use crate::state::SharedState;

mod config;
mod health;
mod logs;
mod system;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/config", get(config::get_config))
        .route("/health", get(health::get_health))
        .route("/system", get(system::get_system))
        .route("/logs", get(logs::list).delete(logs::clear))
        .route("/logs/level", get(logs::get_level).put(logs::put_level))
}
