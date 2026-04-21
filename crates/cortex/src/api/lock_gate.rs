//! Control-lock gate used by mutating handlers.

use axum::http::{HeaderMap, StatusCode};
use axum::Json;

use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

use super::error;

/// Ensures the request holds the operator control lock (423 if another session holds it).
pub fn require_control(
    state: &SharedState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(headers);
    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        return Err(error::err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
        ));
    }
    Ok(())
}
