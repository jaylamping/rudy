//! Shared-bearer-token auth for rudyd.
//!
//! Semantics:
//! - REST side: `Authorization: Bearer <token>` header.
//! - WebTransport side: `?token=<token>` query param on the session URL
//!   (the browser WebTransport API does not expose a request-header hook).

use anyhow::{Context, Result};
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use tracing::warn;

use crate::config::AuthConfig;
use crate::state::AppState;

/// Loads the operator token. Returns `None` if `dev_allow_no_token = true`
/// AND the token file is missing / empty; in that case all auth checks are
/// treated as success and a single warning is logged at startup.
pub fn load_token(cfg: &AuthConfig) -> Result<Option<String>> {
    match std::fs::read_to_string(&cfg.token_file) {
        Ok(s) => {
            let trimmed = s.trim().to_owned();
            if trimmed.is_empty() {
                if cfg.dev_allow_no_token {
                    warn!(
                        path = %cfg.token_file.display(),
                        "rudyd: token file is empty and dev_allow_no_token=true; auth disabled"
                    );
                    Ok(None)
                } else {
                    anyhow::bail!(
                        "auth token file {:?} is empty; set a token or enable dev_allow_no_token",
                        cfg.token_file
                    );
                }
            } else {
                Ok(Some(trimmed))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && cfg.dev_allow_no_token => {
            warn!(
                path = %cfg.token_file.display(),
                "rudyd: token file missing and dev_allow_no_token=true; auth disabled"
            );
            Ok(None)
        }
        Err(e) => Err(anyhow::Error::from(e))
            .with_context(|| format!("reading token file {:?}", cfg.token_file)),
    }
}

/// axum middleware: validates `Authorization: Bearer <token>` on `/api/*`.
pub async fn middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = match state.auth_token.as_deref() {
        None => return Ok(next.run(req).await),
        Some(t) => t,
    };

    let header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "));

    if token.map(|t| constant_time_eq(t.as_bytes(), expected.as_bytes())).unwrap_or(false) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Validate a token from a WebTransport session query string.
pub fn verify_wt_token(state: &AppState, token: Option<&str>) -> bool {
    match state.auth_token.as_deref() {
        None => true,
        Some(expected) => token
            .map(|t| constant_time_eq(t.as_bytes(), expected.as_bytes()))
            .unwrap_or(false),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
