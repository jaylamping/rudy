//! GET / POST / DELETE /api/lock — single-operator control lock.
//!
//! `GET` returns the current holder (or `null` if free). `POST` acquires
//! (or takes over) the lock for the request's `X-Rudy-Session` header.
//! `DELETE` releases the lock if the requester currently holds it.
//!
//! Every transition broadcasts a `SafetyEvent::LockChanged` so other
//! sessions update their UI without polling.

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use serde::Serialize;

use crate::audit::{AuditEntry, AuditResult};
use crate::state::SharedState;
use crate::types::{ApiError, SafetyEvent};
use crate::util::session_from_headers;

#[derive(Debug, Serialize)]
pub struct LockState {
    pub holder: Option<String>,
    pub acquired_at_ms: Option<i64>,
    /// Whether the requester currently holds the lock. Lets the SPA render
    /// "you have control" vs "someone else has control" without parsing the
    /// holder string.
    pub you_hold: bool,
}

fn err(status: StatusCode, error: &str, detail: Option<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
            detail,
        }),
    )
}

pub async fn get_lock(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Json<LockState> {
    let session = session_from_headers(&headers);
    let guard = state.control_lock.read().expect("control_lock poisoned");
    Json(LockState {
        holder: guard.as_ref().map(|h| h.session_id.clone()),
        acquired_at_ms: guard.as_ref().map(|h| h.acquired_at_ms),
        you_hold: matches!(
            (&*guard, session.as_deref()),
            (Some(h), Some(s)) if h.session_id == s
        ),
    })
}

pub async fn acquire_lock(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<LockState>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers).ok_or_else(|| {
        err(
            StatusCode::BAD_REQUEST,
            "missing_session",
            Some("X-Rudy-Session header is required to acquire the lock".into()),
        )
    })?;

    let prev = state.acquire_control(&session);

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: Some(session.clone()),
        remote: None,
        action: if prev.is_some() {
            "control_lock_take_over"
        } else {
            "control_lock_acquire"
        }
        .into(),
        target: None,
        details: serde_json::json!({
            "previous_holder": prev.as_ref().map(|h| h.session_id.clone()),
        }),
        result: AuditResult::Ok,
    });

    let _ = state.safety_event_tx.send(SafetyEvent::LockChanged {
        t_ms: Utc::now().timestamp_millis(),
        holder: Some(session.clone()),
    });

    let guard = state.control_lock.read().expect("control_lock poisoned");
    Ok(Json(LockState {
        holder: guard.as_ref().map(|h| h.session_id.clone()),
        acquired_at_ms: guard.as_ref().map(|h| h.acquired_at_ms),
        you_hold: true,
    }))
}

pub async fn release_lock(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<LockState>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers).ok_or_else(|| {
        err(
            StatusCode::BAD_REQUEST,
            "missing_session",
            Some("X-Rudy-Session header is required to release the lock".into()),
        )
    })?;

    let released = state.release_control(&session);

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: Some(session.clone()),
        remote: None,
        action: "control_lock_release".into(),
        target: None,
        details: serde_json::json!({ "released": released }),
        result: if released {
            AuditResult::Ok
        } else {
            AuditResult::Denied
        },
    });

    if released {
        let _ = state.safety_event_tx.send(SafetyEvent::LockChanged {
            t_ms: Utc::now().timestamp_millis(),
            holder: None,
        });
    }

    Ok(Json(LockState {
        holder: None,
        acquired_at_ms: None,
        you_hold: false,
    }))
}
