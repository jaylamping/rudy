//! REST CRUD for operator reminders. See `crates/cortex/src/reminders.rs`.
//!
//! Routes:
//!   GET    /api/reminders           -> Vec<Reminder>
//!   POST   /api/reminders           -> Reminder           (body: ReminderInput)
//!   PUT    /api/reminders/:id       -> Reminder           (body: ReminderInput)
//!   DELETE /api/reminders/:id       -> 204
//!
//! Mutations are audit-logged through `state.audit` for parity with the
//! motor-param + control routes.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;

use crate::audit::{AuditEntry, AuditResult};
use crate::state::SharedState;
use crate::types::{ApiError, Reminder, ReminderInput};

pub async fn list(State(state): State<SharedState>) -> Json<Vec<Reminder>> {
    Json(state.reminders.list())
}

pub async fn create(
    State(state): State<SharedState>,
    Json(input): Json<ReminderInput>,
) -> Result<(StatusCode, Json<Reminder>), (StatusCode, Json<ApiError>)> {
    let trimmed = input.text.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "empty_text".into(),
                detail: Some("reminder text must be non-empty".into()),
                ..Default::default()
            }),
        ));
    }
    let input = ReminderInput {
        text: trimmed.to_string(),
        ..input
    };
    match state.reminders.create(input.clone()) {
        Ok(r) => {
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: None,
                remote: None,
                action: "reminder_create".into(),
                target: Some(r.id.clone()),
                details: serde_json::json!({ "text": r.text, "due_at": r.due_at }),
                result: AuditResult::Ok,
            });
            Ok((StatusCode::CREATED, Json(r)))
        }
        Err(e) => Err(persist_error("reminder_create", None, &input, &e)),
    }
}

pub async fn update(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(input): Json<ReminderInput>,
) -> Result<Json<Reminder>, (StatusCode, Json<ApiError>)> {
    let trimmed = input.text.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "empty_text".into(),
                detail: Some("reminder text must be non-empty".into()),
                ..Default::default()
            }),
        ));
    }
    let input = ReminderInput {
        text: trimmed.to_string(),
        ..input
    };
    match state.reminders.update(&id, input.clone()) {
        Ok(Some(r)) => {
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: None,
                remote: None,
                action: "reminder_update".into(),
                target: Some(id),
                details: serde_json::json!({ "text": r.text, "done": r.done }),
                result: AuditResult::Ok,
            });
            Ok(Json(r))
        }
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "unknown_reminder".into(),
                detail: Some(format!("no reminder with id={id}")),
                ..Default::default()
            }),
        )),
        Err(e) => Err(persist_error("reminder_update", Some(&id), &input, &e)),
    }
}

pub async fn delete_one(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    match state.reminders.delete(&id) {
        Ok(true) => {
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: None,
                remote: None,
                action: "reminder_delete".into(),
                target: Some(id),
                details: serde_json::json!({}),
                result: AuditResult::Ok,
            });
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "unknown_reminder".into(),
                detail: Some(format!("no reminder with id={id}")),
                ..Default::default()
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "persist_failed".into(),
                detail: Some(format!("{e:#}")),
                ..Default::default()
            }),
        )),
    }
}

fn persist_error(
    action: &str,
    id: Option<&str>,
    input: &ReminderInput,
    e: &anyhow::Error,
) -> (StatusCode, Json<ApiError>) {
    tracing::error!(action, ?id, ?input, error = %e, "reminder persist failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error: "persist_failed".into(),
            detail: Some(format!("{e:#}")),
            ..Default::default()
        }),
    )
}
