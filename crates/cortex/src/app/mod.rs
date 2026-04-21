//! Application runtime: shared daemon state, lock helpers, and bootstrap (see plan).

pub mod bootstrap;
pub mod state;

pub use state::{AppState, SharedState};
