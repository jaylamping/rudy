//! GET /api/health - lightweight daemon health for local watchdogs.
//!
//! This endpoint is intentionally cheap and loopback-focused. It answers:
//! - is the daemon process serving requests,
//! - was the SPA shell embedded into this binary,
//! - and is CAN configured in a way this build can satisfy?

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub healthy: bool,
    pub spa_embedded: bool,
    pub can_mock: bool,
    pub can_ready: bool,
}

pub async fn get_health(State(state): State<SharedState>) -> (StatusCode, Json<HealthResponse>) {
    let spa_embedded = crate::http::spa_present();
    let can_mock = state.cfg.can.mock;
    let can_ready = can_mock || state.real_can.is_some();
    let response = build_health_response(spa_embedded, can_mock, can_ready);
    let status = if response.healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(response))
}

fn build_health_response(spa_embedded: bool, can_mock: bool, can_ready: bool) -> HealthResponse {
    let healthy = spa_embedded;
    let status = if healthy { "ok" } else { "degraded" };
    HealthResponse {
        status,
        healthy,
        spa_embedded,
        can_mock,
        can_ready,
    }
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
