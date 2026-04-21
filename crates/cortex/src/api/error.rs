//! Shared JSON error helper for handlers.

use axum::{http::StatusCode, Json};

use crate::types::ApiError;

/// Standard `(status, ApiError)` tuple for `Result::Err` from Axum handlers.
pub fn err(
    status: StatusCode,
    error: &str,
    detail: Option<String>,
) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
            detail,
            ..Default::default()
        }),
    )
}
