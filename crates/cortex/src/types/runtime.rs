//! Runtime FSM status (`GET /api/runtime`, WebTransport `runtime_status`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// High-level daemon runtime state from ADR-0008.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Passive,
    Commissioning,
    Ready,
    Homing,
    Holding,
    ManualJog,
    PrimitiveRunning,
    Faulted,
    EStop,
}

/// One active motion summarized for runtime-status consumers.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct RuntimeMotion {
    pub run_id: String,
    pub role: String,
    pub kind: String,
    pub started_at_ms: i64,
}

/// A condition explaining why the runtime is not simply `ready`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct RuntimeBlocker {
    pub kind: String,
    pub role: Option<String>,
    pub message: String,
}

/// Current read-only runtime FSM snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct RuntimeStatus {
    pub t_ms: i64,
    pub state: RuntimeState,
    /// Stable snake_case reason for the chosen state.
    pub reason: String,
    pub can_mock: bool,
    pub can_ready: bool,
    /// Informational only in this first ADR-0008 implementation. Later gate
    /// PRs will make this field line up with enforcement.
    pub can_accept_motion: bool,
    pub active_motions: Vec<RuntimeMotion>,
    pub enabled_roles: Vec<String>,
    pub position_hold_roles: Vec<String>,
    pub blockers: Vec<RuntimeBlocker>,
}
