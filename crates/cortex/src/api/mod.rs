//! REST API router.
//!
//! All routes return JSON. Mutating endpoints audit-log their inputs and
//! (when applicable) range-check against the actuator spec.

use axum::Router;

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
        .with_state(state)
}
