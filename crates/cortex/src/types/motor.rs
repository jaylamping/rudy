//! Motor, params, and API error wire types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// GET /api/motors — list summary.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct MotorSummary {
    pub role: String,
    pub can_bus: String,
    pub can_id: u8,
    pub firmware_version: Option<String>,
    pub verified: bool,
    pub present: bool,
    /// Daemon's best-effort view of whether torque is currently enabled.
    pub enabled: bool,
    pub travel_limits: Option<crate::inventory::TravelLimits>,
    /// Target angle (radians) for boot-time auto-home. `None` uses 0.0 rad;
    /// when set, must stay within `travel_limits`.
    #[serde(default)]
    pub predefined_home_rad: Option<f32>,
    /// Per-actuator override for home-ramp nominal speed (rad/s). `None`
    /// follows [`default_homing_speed_rad_s`].
    #[serde(default)]
    pub homing_speed_rad_s: Option<f32>,
    /// Global effective home-ramp speed (rad/s) from `cortex.toml` for SPA display.
    pub default_homing_speed_rad_s: f32,
    pub latest: Option<MotorFeedback>,
    /// Age of `latest` in ms at serialization time. This can be refreshed by
    /// type-17 fallback polling, so use `type2_age_ms` for motion safety.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub feedback_age_ms: Option<i64>,
    /// Age of the last high-rate type-2 position frame in ms. `None` means no
    /// type-2 frame has been decoded for this motor since daemon start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub type2_age_ms: Option<i64>,
    /// Per-power-cycle gate state. UI renders a colored badge driven off
    /// the variant; OutOfBand carries enough detail to display
    /// "X.X deg outside [Y.Y, Z.Z]" without a second roundtrip.
    pub boot_state: crate::boot_state::BootState,
    /// Limb assignment, if any (`left_arm`, `right_leg`, etc.). None for
    /// ungrouped motors that haven't been assigned via the inventory tab.
    pub limb: Option<String>,
    /// Canonical position in the kinematic chain. None for ungrouped motors.
    pub joint_kind: Option<crate::limb::JointKind>,
    /// Writable params where live RAM differs from `inventory.desired_params`.
    #[serde(default)]
    pub drifted_param_count: u32,
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
    /// `true` for entries sourced from `spec.firmware_limits` (the
    /// SPA shows these in the writable Parameters table and the
    /// `PUT /api/motors/:role/params/:name` handler accepts writes
    /// to them); `false` for `spec.observables` (read-only). The
    /// SPA used to derive this from `hardware_range.is_some()`,
    /// but several writable params (`run_mode`, `can_timeout`,
    /// `zero_sta`, `damper`, `add_offset`) intentionally have no
    /// numeric range — they're enums or counters — and were being
    /// misclassified as observables. This flag tracks the spec
    /// section directly so any future writable-without-range param
    /// stays writable in the UI.
    pub writable: bool,
    /// From `inventory.desired_params` for writable params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub desired: Option<serde_json::Value>,
    /// Present when live value differs from desired (writable limits only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub drift: Option<ParamDrift>,
}

/// Live vs desired mismatch for a single param (writable firmware limits).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ParamDrift {
    pub live: serde_json::Value,
    pub desired: serde_json::Value,
}

/// PUT /api/motors/:role/params/:index body.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ParamWrite {
    pub value: serde_json::Value,
    /// Ignored: writes always persist to actuator flash (type-22) and `inventory.desired_params`.
    #[serde(default)]
    pub save_after: bool,
}

/// POST /api/motors/:role/params/sync body.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ParamSyncRequest {
    /// When `None`, sync every param that currently has drift.
    #[serde(default)]
    #[ts(optional)]
    pub names: Option<Vec<String>>,
}

/// One motor contributing to a [`limb_quarantined`](ApiError::error) refusal.
#[derive(Debug, Clone, Serialize, Deserialize, TS, Default)]
#[ts(export, export_to = "./")]
pub struct LimbQuarantineMotor {
    pub role: String,
    /// [`BootState`](crate::boot_state::BootState) kind string (`snake_case`).
    pub state_kind: String,
}

/// Standard error envelope for API responses.
#[derive(Debug, Clone, Serialize, Deserialize, TS, Default)]
#[ts(export, export_to = "./")]
pub struct ApiError {
    pub error: String,
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub failed_motors: Option<Vec<LimbQuarantineMotor>>,
}
