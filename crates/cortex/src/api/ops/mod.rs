//! Operator actions: estop, reminders.

use axum::{
    routing::{get, post, put},
    Router,
};

use crate::state::SharedState;

mod estop;
mod reminders;
pub mod restart;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/estop", post(estop::estop))
        .route("/restart", post(restart::restart))
        .route("/reminders", get(reminders::list).post(reminders::create))
        .route(
            "/reminders/:id",
            put(reminders::update).delete(reminders::delete_one),
        )
}
