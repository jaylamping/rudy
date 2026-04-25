//! REST API router.
//!
//! All routes return JSON. Mutating endpoints audit-log their inputs and
//! (when applicable) range-check against the actuator spec.

use axum::{
    body::Body,
    http::{header, HeaderValue, Request},
    middleware::{self, Next},
    response::Response,
    Router,
};

use crate::state::SharedState;

pub mod error;
pub mod inventory;
pub mod lock_gate;
pub mod meta;
pub mod motion;
pub mod motors;
pub mod ops;

pub fn router(state: SharedState) -> Router<SharedState> {
    Router::new()
        .merge(meta::router())
        .merge(inventory::router())
        .merge(motors::router())
        .merge(motion::router())
        .merge(ops::router())
        .layer(middleware::from_fn(no_store))
        .with_state(state)
}

async fn no_store(req: Request<Body>, next: Next) -> Response {
    let mut res = next.run(req).await;
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}
