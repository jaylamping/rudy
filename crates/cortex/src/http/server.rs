//! Axum plaintext-HTTP listener: REST API router + embedded-SPA middleware.
//!
//! No TLS, no auth. Two reasons that's safe:
//!
//! 1. On the Pi the listener binds `127.0.0.1` only, so the only client that
//!    can reach it is a process on the same host — namely `tailscale serve`,
//!    which terminates TLS with the auto-managed Tailscale Let's Encrypt cert
//!    and proxies decrypted requests in. The tailnet (and only the tailnet)
//!    sees `https://rudy-pi/`; nothing on the LAN can reach `:8443` directly.
//! 2. In dev, Vite proxies `/api/*` to `http://127.0.0.1:8443` from
//!    `http://localhost:5173`, same machine, no TLS in either hop.
//!
//! This used to terminate TLS itself with rustls; see ADR-0004 addendum for
//! why we moved cert handling into Tailscale.
//!
//! Everything under `/api/*` is the JSON REST surface; everything else falls
//! through to a `rust-embed`-backed handler that serves
//! `crates/cortex/static/` (copied from `link/dist/` at build time) with
//! SPA-style fallback to `index.html` for client-side routing.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{routing::get, Router};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::api;
use crate::state::SharedState;
use tokio::sync::oneshot;

use super::spa;

pub async fn run(state: SharedState, ready_tx: Option<oneshot::Sender<()>>) -> Result<()> {
    let bind: SocketAddr = state
        .cfg
        .http
        .bind
        .parse()
        .with_context(|| format!("parsing http.bind {:?}", state.cfg.http.bind))?;

    let api_routes = api::router(state.clone());

    let app = Router::new()
        .nest("/api", api_routes)
        .route("/", get(spa::index_handler))
        .fallback(spa::static_handler)
        .with_state(state.clone())
        .layer(TraceLayer::new_for_http());

    info!(%bind, "cortex http listener up (plaintext; TLS terminated upstream by `tailscale serve` on the Pi)");
    if let Some(tx) = ready_tx {
        let _ = tx.send(());
    }
    axum_server::bind(bind)
        .serve(app.into_make_service())
        .await
        .context("axum_server serve")?;
    Ok(())
}

pub(crate) use spa::spa_present;
