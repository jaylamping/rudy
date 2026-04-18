//! Wire types shared between rudydae and the `link` SPA.
//!
//! Every type here has `#[derive(TS)] #[ts(export, export_to = "...")]`, so
//! `cargo test -p rudydae export_bindings` regenerates `link/src/lib/types/*.ts`.
//! `crates/.cargo/config.toml` sets `TS_RS_EXPORT_DIR` so outputs land next to the SPA.
//! Run `python scripts/fix-ts-rs-imports.py` (or `npm run gen:types` in `link/`) to fix serde_json paths. See
//! <https://github.com/Aleph-Alpha/ts-rs>.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// GET /api/config — what the UI needs to bootstrap.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ServerConfig {
    pub version: String,
    pub actuator_model: String,
    pub webtransport: WebTransportAdvert,
    pub features: ServerFeatures,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct WebTransportAdvert {
    pub enabled: bool,
    /// Fully-qualified URL the browser should open. Example:
    /// `https://rudy.your-tailnet.ts.net:4433/wt`.
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ServerFeatures {
    pub mock_can: bool,
    pub require_verified: bool,
}

/// GET /api/motors — list summary.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct MotorSummary {
    pub role: String,
    pub can_bus: String,
    pub can_id: u8,
    pub firmware_version: Option<String>,
    pub verified: bool,
    pub latest: Option<MotorFeedback>,
}

/// One snapshot of telemetry for a motor. Sent:
/// - as JSON from `GET /api/motors/:role/feedback` (polled),
/// - as CBOR from WebTransport datagrams (pushed).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct MotorFeedback {
    /// Milliseconds since unix epoch, for trivial client-side ordering.
    pub t_ms: i64,
    pub role: String,
    pub can_id: u8,
    pub mech_pos_rad: f32,
    pub mech_vel_rad_s: f32,
    pub torque_nm: f32,
    pub vbus_v: f32,
    pub temp_c: f32,
    pub fault_sta: u32,
    pub warn_sta: u32,
}

/// GET /api/motors/:role/params — full catalog snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ParamSnapshot {
    pub role: String,
    pub values: BTreeMap<String, ParamValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ParamValue {
    pub name: String,
    pub index: u16,
    #[serde(rename = "type")]
    pub ty: String,
    pub units: Option<String>,
    pub value: serde_json::Value,
    pub hardware_range: Option<[f32; 2]>,
}

/// PUT /api/motors/:role/params/:index body.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ParamWrite {
    pub value: serde_json::Value,
    /// If `true`, rudydae also issues the type-22 save after the write. If
    /// `false` (default), the value lives in RAM and `POST /api/motors/:role/save`
    /// is required to persist it.
    #[serde(default)]
    pub save_after: bool,
}

/// Standard error envelope for API responses.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ApiError {
    pub error: String,
    pub detail: Option<String>,
}

/// GET /api/system - host metrics for the operator-console dashboard.
///
/// Linux real values come from `/proc` + `/sys` + (on the Pi) `vcgencmd`;
/// when `cfg.can.mock == true` or running on non-Linux, fields are
/// slowly-varying mock numbers and `is_mock = true`. See `system.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SystemSnapshot {
    /// Wallclock at sample time, ms since unix epoch.
    pub t_ms: i64,
    pub cpu_pct: f32,
    /// 1, 5, 15-minute load average from `/proc/loadavg`.
    pub load: [f32; 3],
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub temps_c: SystemTemps,
    pub throttled: SystemThrottled,
    pub uptime_s: u64,
    pub hostname: String,
    pub kernel: String,
    /// True when values are synthetic (no Linux host or `cfg.can.mock = true`).
    pub is_mock: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SystemTemps {
    pub cpu: Option<f32>,
    pub gpu: Option<f32>,
}

/// Pi-specific power/thermal throttling state. `now` and `ever` are derived
/// from `vcgencmd get_throttled` bits (0/2 -> now, 16/18 -> ever).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SystemThrottled {
    pub now: bool,
    pub ever: bool,
    pub raw_hex: Option<String>,
}

/// One operator reminder. File-backed in `.rudyd/reminders.json`.
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

/// WebTransport subscription request (sent on a bidirectional stream by the
/// client right after session open).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(dead_code)] // Wire shape for WebTransport; parsed in wt/ when Phase 2 lands.
pub enum WtSubscribe {
    /// High-rate feedback datagrams for the listed motor roles (empty = all).
    Feedback { roles: Vec<String> },
    /// Fault / warn events as reliable stream messages.
    Faults,
    /// Journald tail as reliable stream messages.
    Logs { unit: Option<String> },
}
