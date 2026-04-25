//! Meta routes: config, health, system, logs.

use axum::{routing::get, routing::post, routing::put, Router};

use crate::state::SharedState;

mod config;
mod health;
mod logs;
mod settings;
mod system;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/config", get(config::get_config))
        .route("/health", get(health::get_health))
        .route("/system", get(system::get_system))
        .route("/logs", get(logs::list).delete(logs::clear))
        .route("/logs/level", get(logs::get_level).put(logs::put_level))
        .route("/settings", get(settings::get_all))
        .route("/settings/reset", post(settings::post_reset))
        .route("/settings/reseed", post(settings::post_reseed))
        .route("/settings/recovery/ack", post(settings::post_recovery_ack))
        .route(
            "/settings/profiles",
            get(settings::get_profiles).post(settings::post_create_profile),
        )
        // Single-segment name only (e.g. `bench`); catch-all must end the path in axum 0.7.
        .route(
            "/settings/profiles/apply/:name",
            post(settings::post_apply_profile),
        )
        // One URL segment per key (e.g. `safety.require_verified`); no `/*` catch-all.
        .route("/settings/:key", put(settings::put_one))
}
