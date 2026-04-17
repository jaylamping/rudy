//! Axum HTTPS listener: REST API router + embedded-SPA middleware layer.
//!
//! Everything under `/api/*` is gated by the shared-token auth middleware.
//! Everything else falls through to a `rust-embed`-backed handler that
//! serves `crates/rudyd/static/` (copied from `link/dist/` at build time)
//! with SPA-style fallback to `index.html` for client-side routing.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::{header, StatusCode, Uri},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::{Embed as _, RustEmbed};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use crate::api;
use crate::auth;
use crate::state::SharedState;

#[derive(RustEmbed)]
#[folder = "static/"]
struct Assets;

pub async fn run(state: SharedState) -> Result<()> {
    let bind: SocketAddr = state
        .cfg
        .http
        .bind
        .parse()
        .with_context(|| format!("parsing http.bind {:?}", state.cfg.http.bind))?;

    let api_routes = api::router(state.clone()).layer(middleware::from_fn_with_state(
        state.clone(),
        auth::middleware,
    ));

    let app = Router::new()
        .nest("/api", api_routes)
        .route("/", get(index_handler))
        .fallback(static_handler)
        .with_state(state.clone())
        .layer(TraceLayer::new_for_http());

    if state.cfg.http.tls.enabled {
        let (cert, key) = (
            state
                .cfg
                .http
                .tls
                .cert_path
                .clone()
                .context("http.tls.enabled=true but http.tls.cert_path not set")?,
            state
                .cfg
                .http
                .tls
                .key_path
                .clone()
                .context("http.tls.enabled=true but http.tls.key_path not set")?,
        );
        let tls_cfg = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
            .await
            .context("loading TLS cert/key for axum")?;
        info!(%bind, "rudyd https listener up");
        axum_server::bind_rustls(bind, tls_cfg)
            .serve(app.into_make_service())
            .await
            .context("axum_server serve")?;
    } else {
        warn!(%bind, "rudyd http listener up (TLS disabled - dev only)");
        axum_server::bind(bind)
            .serve(app.into_make_service())
            .await
            .context("axum_server serve")?;
    }
    Ok(())
}

async fn index_handler(State(state): State<SharedState>) -> Response {
    // Auth middleware already ran on `/`; bypass it for the SPA shell so the
    // token login screen can render.
    let _ = state;
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
