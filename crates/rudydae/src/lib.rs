//! `rudydae` library surface.
//!
//! The crate ships a binary (`src/main.rs`) and a library. The library exists
//! so integration tests under `tests/` can build the same `axum::Router` and
//! `AppState` the binary uses, without having to spawn a real OS process or
//! TLS listener. See `tests/api_contract.rs` for the canonical consumer.
//!
//! Modules are re-exported as-is for tests; nothing here is part of a stable
//! public API — downstream code should still treat rudydae as a binary.

pub mod api;
pub mod audit;
pub mod boot_state;
pub mod can;
pub mod config;
pub mod inventory;
pub mod limb;
pub mod reminders;
pub mod server;
pub mod spec;
pub mod state;
pub mod system;
pub mod telemetry;
pub mod types;
pub mod util;
pub mod wt;
pub mod wt_router;

pub use state::{AppState, SharedState};

/// Build the same axum `Router` the daemon serves, parameterised over a
/// pre-built `SharedState`. Useful for `tower::ServiceExt::oneshot`-style
/// integration tests that don't want to bind a TCP socket.
///
/// Mirrors `server::run`'s composition (`/api` + SPA fallback) but without the
/// TLS / `axum_server::bind*` plumbing.
pub fn build_app(state: SharedState) -> axum::Router {
    axum::Router::new()
        .nest("/api", api::router(state.clone()))
        .with_state(state)
}
