//! HTTP helpers shared by REST handlers.

use axum::http::HeaderMap;

/// Header the SPA mints per browser tab and sends on every mutating request.
/// The daemon's single-operator lock keys off it; missing header ≡ "no
/// session id supplied" which the lock check treats as never-the-holder.
pub const SESSION_HEADER: &str = "X-Rudy-Session";

/// Extract the per-tab session id from a request's headers, if present.
pub fn session_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
