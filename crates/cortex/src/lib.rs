//! `cortex` library surface.
//!
//! The crate ships a binary (`src/main.rs`) and a library. The library exists
//! so integration tests under `tests/` can build the same `axum::Router` and
//! `AppState` the binary uses, without having to spawn a real OS process or
//! TLS listener. See `tests/api/*.rs` and `tests/can/*.rs` (registered in `Cargo.toml` as `[[test]]`) for integration suites.
//!
//! Modules are re-exported as-is for tests; nothing here is part of a stable
//! public API — downstream code should still treat cortex as a binary.

pub mod api;
pub mod app;
pub mod can;
pub mod config;
pub mod hardware;
pub mod http;
pub mod motion;
pub mod observability;
pub mod types;
pub mod util;
pub mod webtransport;

pub use can::discovery;
pub use hardware::boot::orchestrator as boot_orchestrator;
pub use hardware::boot::state as boot_state;
pub use hardware::inventory;
pub use hardware::limb;
pub use hardware::limb::health as limb_health;
pub use hardware::spec;
pub use observability::system;
pub use observability::{audit, log_layer, log_store, reminders, telemetry};
pub use webtransport::client_frames as wt_client;
pub use webtransport::listener as wt;
pub use webtransport::session as wt_router;

pub use app::state;
pub use app::{AppState, SharedState};

/// Build the same axum `Router` the daemon serves, parameterised over a
/// pre-built `SharedState`. Useful for `tower::ServiceExt::oneshot`-style
/// integration tests that don't want to bind a TCP socket.
///
/// Mirrors `http::run`'s composition (`/api` + SPA fallback) but without the
/// TLS / `axum_server::bind*` plumbing.
pub fn build_app(state: SharedState) -> axum::Router {
    axum::Router::new()
        .nest("/api", api::router(state.clone()))
        .with_state(state)
}
