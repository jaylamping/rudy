//! Wire types shared between rudyd and the `link` SPA.
//!
//! Every type here has `#[derive(TS)] #[ts(export, export_to = "...")]`, so
//! `cargo test` regenerates `link/src/api/generated/*.ts`. See
//! <https://github.com/Aleph-Alpha/ts-rs>.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// GET /api/config — what the UI needs to bootstrap.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../link/src/api/generated/")]
pub struct ServerConfig {
    pub version: String,
    pub actuator_model: String,
    pub webtransport: WebTransportAdvert,
    pub features: ServerFeatures,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../link/src/api/generated/")]
pub struct WebTransportAdvert {
    pub enabled: bool,
    /// Fully-qualified URL the browser should open. Example:
    /// `https://rudy.your-tailnet.ts.net:4433/wt`.
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../link/src/api/generated/")]
pub struct ServerFeatures {
    pub mock_can: bool,
    pub require_verified: bool,
}

/// GET /api/motors — list summary.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../link/src/api/generated/")]
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
#[ts(export, export_to = "../../link/src/api/generated/")]
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
#[ts(export, export_to = "../../link/src/api/generated/")]
pub struct ParamSnapshot {
    pub role: String,
    pub values: BTreeMap<String, ParamValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../link/src/api/generated/")]
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
#[ts(export, export_to = "../../link/src/api/generated/")]
pub struct ParamWrite {
    pub value: serde_json::Value,
    /// If `true`, rudyd also issues the type-22 save after the write. If
    /// `false` (default), the value lives in RAM and `POST /api/motors/:role/save`
    /// is required to persist it.
    #[serde(default)]
    pub save_after: bool,
}

/// Standard error envelope for API responses.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../link/src/api/generated/")]
pub struct ApiError {
    pub error: String,
    pub detail: Option<String>,
}

/// WebTransport subscription request (sent on a bidirectional stream by the
/// client right after session open).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../link/src/api/generated/")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WtSubscribe {
    /// High-rate feedback datagrams for the listed motor roles (empty = all).
    Feedback { roles: Vec<String> },
    /// Fault / warn events as reliable stream messages.
    Faults,
    /// Journald tail as reliable stream messages.
    Logs { unit: Option<String> },
}
