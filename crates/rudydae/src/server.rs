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
//! `crates/rudydae/static/` (copied from `link/dist/` at build time) with
//! SPA-style fallback to `index.html` for client-side routing.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::RustEmbed;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::api;
use crate::state::SharedState;
use tokio::sync::oneshot;

#[derive(RustEmbed)]
#[folder = "static/"]
struct Assets;

pub(crate) fn spa_present() -> bool {
    Assets::get("index.html").is_some()
}

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
        .route("/", get(index_handler))
        .fallback(static_handler)
        .with_state(state.clone())
        .layer(TraceLayer::new_for_http());

    info!(%bind, "rudydae http listener up (plaintext; TLS terminated upstream by `tailscale serve` on the Pi)");
    if let Some(tx) = ready_tx {
        let _ = tx.send(());
    }
    axum_server::bind(bind)
        .serve(app.into_make_service())
        .await
        .context("axum_server serve")?;
    Ok(())
}

async fn index_handler(State(_state): State<SharedState>) -> Response {
    serve_asset("index.html")
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.is_empty() {
        return serve_asset("index.html");
    }
    if Assets::get(path).is_some() {
        serve_asset(path)
    } else {
        // Single-page-app routing: unknown paths get index.html so the
        // client-side router can take over.
        serve_asset("index.html")
    }
}

fn serve_asset(path: &str) -> Response {
    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
