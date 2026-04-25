//! `GET/PUT /api/settings` contract.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Top-level `GET /api/settings`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SettingsGetResponse {
    /// When the SQLite runtime store is off, `PUT` is rejected.
    pub runtime_db_enabled: bool,
    /// Set after corrupt-DB re-seed until `POST /api/settings/recovery/ack`.
    pub recovery_pending: bool,
    pub entries: Vec<SettingEntry>,
}

/// One registered tunable (or read-only) key.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SettingEntry {
    pub key: String,
    pub label: String,
    pub description: String,
    pub category: String,
    /// Display hint: `bool`, `f32`, `u32`, `u64`, `option_f32`
    pub value_kind: String,
    pub unit: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Value from TOML seed (same as first-boot import / reset target).
    pub seed: Value,
    /// Live merged value (memory).
    pub effective: Value,
    /// Row present in `settings_kv` when the runtime DB is on.
    pub in_db: bool,
    /// Differs from seed when `in_db` (operator or import changed this key).
    pub dirty: bool,
    /// Wire apply semantics for the SPA.
    pub apply_mode: SettingsApplyMode,
    /// `PUT` allowed (requires control lock; may also need motors stopped).
    pub editable: bool,
    /// Shown when `editable` is false.
    pub read_only_reason: Option<String>,
    /// Gating hint for the operator (motion stopped on the bus).
    pub requires_motors_stopped: bool,
}

/// Apply semantics (subset of the migration plan; extend as HIL rules firm up).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(rename_all = "snake_case")]
pub enum SettingsApplyMode {
    ReadOnly,
    /// Safe to merge into memory; motion code picks up on next use.
    RuntimeImmediate,
    /// Value stored; full effect may need daemon restart (telemetry poll thread).
    RequiresRestart,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct PutSettingRequest {
    /// JSON `Value` appropriate for the key (bool, number, or null for optional float).
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct PutSettingResponse {
    pub ok: bool,
    pub key: String,
    pub effective: Value,
    pub apply_mode: SettingsApplyMode,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SettingsRecoveryAckResponse {
    pub ok: bool,
    pub recovery_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SettingsResetResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SettingsProfileInfo {
    pub name: String,
    pub key_count: usize,
    /// Non-cryptographic fingerprint of stored JSON (for table display).
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SettingsProfilesListResponse {
    pub profiles: Vec<SettingsProfileInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SettingsProfileCreateRequest {
    pub name: String,
    /// Subset of `safety.*` / `telemetry.*` values to snapshot as a profile.
    pub values: std::collections::BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SettingsProfileCreateResponse {
    pub ok: bool,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SettingsProfileApplyResponse {
    pub ok: bool,
    pub name: String,
    pub note: Option<String>,
}
