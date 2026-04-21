//! Embedded SPA (`static/`) asset serving.

use axum::{
    extract::State,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

use crate::state::SharedState;

#[derive(RustEmbed)]
#[folder = "static/"]
struct Assets;

pub(crate) fn spa_present() -> bool {
    Assets::get("index.html").is_some()
}

pub(super) async fn index_handler(State(_state): State<SharedState>) -> Response {
    serve_asset("index.html")
}

pub(super) async fn static_handler(uri: Uri) -> Response {
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
