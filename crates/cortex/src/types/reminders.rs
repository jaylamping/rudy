//! Operator reminder wire types.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One operator reminder. File-backed in `.cortex/reminders.json`.
/// Created/edited/deleted via `/api/reminders[/:id]`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct Reminder {
    pub id: String,
    pub text: String,
    /// Optional ISO 8601 due date; the UI renders relative ("in 2h", "overdue").
    pub due_at: Option<String>,
    pub done: bool,
    /// Wallclock at creation, ms since unix epoch.
    pub created_ms: i64,
}

/// POST /api/reminders body and PUT /api/reminders/:id body.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ReminderInput {
    pub text: String,
    pub due_at: Option<String>,
    #[serde(default)]
    pub done: bool,
}
