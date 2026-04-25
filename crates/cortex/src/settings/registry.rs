//! Registry of tunable `safety.*` / `telemetry.*` keys (drives GET /api/settings).

use serde_json::{json, Value as Json};

use crate::config::{Config, SafetyConfig, TelemetryConfig};

use super::merge::file_defaults_to_kv;

/// How a value takes effect in the running daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireApplyMode {
    ReadOnly,
    /// Updated in memory; motion reads next loop / command.
    RuntimeImmediate,
    /// Requires process restart to fully apply (e.g. telemetry poll loop).
    RequiresRestart,
}

/// Static description for one settings key.
#[derive(Debug, Clone, Copy)]
pub struct SettingDef {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// `"safety"` or `"telemetry"`
    pub category: &'static str,
    pub unit: Option<&'static str>,
    /// JSON Schema-ish hint for the SPA.
    pub value_kind: &'static str,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// When `true`, PUT refuses unless all motors are stopped (`state.enabled` empty).
    pub requires_motors_stopped: bool,
    pub apply_mode: WireApplyMode,
    /// When runtime DB is off, all keys are read-only from the operator's perspective.
    pub tunable: bool,
}

/// Every key stored in `settings_kv` and shown in the Settings UI.
pub const ALL: &[SettingDef] = &[
    SettingDef {
        key: "safety.require_verified",
        label: "Require verified motors",
        description: "Refuse motion on motors that are not commission-verified when true.",
        category: "safety",
        unit: None,
        value_kind: "bool",
        min: None,
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.boot_max_step_rad",
        label: "Boot max step (rad)",
        description: "Largest position step allowed at boot before homing.",
        category: "safety",
        unit: Some("rad"),
        value_kind: "f32",
        min: Some(0.0),
        max: Some(1.0),
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.step_size_rad",
        label: "Planner step (rad)",
        description: "Default discrete step for motion / home-ramp chunks.",
        category: "safety",
        unit: Some("rad"),
        value_kind: "f32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.tick_interval_ms",
        label: "Homer tick (ms)",
        description: "Wall-clock period between home-ramp control iterations.",
        category: "safety",
        unit: Some("ms"),
        value_kind: "u32",
        min: Some(1.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.homing_speed_rad_s",
        label: "Homing speed (rad/s)",
        description: "Default homing speed; null uses firmware/role default.",
        category: "safety",
        unit: Some("rad/s"),
        value_kind: "option_f32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.tracking_error_max_rad",
        label: "Tracking error max (rad)",
        description: "Operator motion tracking-error ceiling.",
        category: "safety",
        unit: Some("rad"),
        value_kind: "f32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.tracking_error_grace_ticks",
        label: "Tracking error grace (ticks)",
        description: "Ticks to tolerate tracking error after a setpoint before abort.",
        category: "safety",
        unit: Some("ticks"),
        value_kind: "u32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.tracking_freshness_max_age_ms",
        label: "Tracking freshness max (ms)",
        description: "Max age of feedback sample for tracking gate.",
        category: "safety",
        unit: Some("ms"),
        value_kind: "u64",
        min: Some(1.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.tracking_error_debounce_ticks",
        label: "Tracking error debounce (ticks)",
        description: "Consecutive OOB ticks before motion abort (tracking).",
        category: "safety",
        unit: Some("ticks"),
        value_kind: "u32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.band_violation_debounce_ticks",
        label: "Band violation debounce (ticks)",
        description: "Consecutive ticks outside travel band before abort.",
        category: "safety",
        unit: Some("ticks"),
        value_kind: "u32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.boot_tracking_error_max_rad",
        label: "Boot tracking error max (rad)",
        description: "Larger tracking budget for boot orchestrator / cold start.",
        category: "safety",
        unit: Some("rad"),
        value_kind: "f32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.target_tolerance_rad",
        label: "Target tolerance (rad)",
        description: "Position band considered “at target” for dwell / hold checks.",
        category: "safety",
        unit: Some("rad"),
        value_kind: "f32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.target_dwell_ticks",
        label: "Target dwell (ticks)",
        description: "Ticks to remain within tolerance before success.",
        category: "safety",
        unit: Some("ticks"),
        value_kind: "u32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.target_dwell_max_vel_rad_s",
        label: "Dwell max velocity (rad/s)",
        description: "Velocity gate for dwell; +∞ disables the gate.",
        category: "safety",
        unit: Some("rad/s"),
        value_kind: "f32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.homer_timeout_ms",
        label: "Homer wall timeout (ms)",
        description: "Max wall-clock for one homer run before timeout abort.",
        category: "safety",
        unit: Some("ms"),
        value_kind: "u32",
        min: Some(1_000.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.max_feedback_age_ms",
        label: "Max feedback age (ms)",
        description: "Telemetry staleness cap for control / jog / gating.",
        category: "safety",
        unit: Some("ms"),
        value_kind: "u64",
        min: Some(1.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.commission_readback_tolerance_rad",
        label: "Commission readback ε (rad)",
        description: "Offset read vs stored match tolerance (boot orchestrator).",
        category: "safety",
        unit: Some("rad"),
        value_kind: "f32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.auto_home_on_boot",
        label: "Auto home on boot",
        description: "Run boot orchestrator auto-home for commissioned motors when true.",
        category: "safety",
        unit: None,
        value_kind: "bool",
        min: None,
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.scan_on_boot",
        label: "Scan on boot",
        description: "Hint for hardware scan on startup (where implemented).",
        category: "safety",
        unit: None,
        value_kind: "bool",
        min: None,
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.hold_kp_nm_per_rad",
        label: "Hold Kp (N·m/rad)",
        description: "MIT / spring hold proportional gain (post-homing).",
        category: "safety",
        unit: None,
        value_kind: "f32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.hold_kd_nm_s_per_rad",
        label: "Hold Kd (N·m·s/rad)",
        description: "MIT / spring hold derivative gain (post-homing).",
        category: "safety",
        unit: None,
        value_kind: "f32",
        min: Some(0.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.motion_backend",
        label: "Motion backend",
        description:
            "velocity = SPD_REF stream; mit = streaming MIT OperationCtrl for jog/sweep/wave.",
        category: "safety",
        unit: None,
        value_kind: "string",
        min: None,
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.mit_command_rate_hz",
        label: "MIT command rate (Hz)",
        description: "Nominal MIT streaming rate (controller tick matches telemetry).",
        category: "safety",
        unit: Some("Hz"),
        value_kind: "f32",
        min: Some(1.0),
        max: Some(500.0),
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.mit_max_angle_step_rad",
        label: "MIT max step (rad/tick)",
        description: "Per-tick shortest-path cap for MIT motion targets.",
        category: "safety",
        unit: Some("rad"),
        value_kind: "f32",
        min: Some(1e-4),
        max: Some(1.0),
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.mit_lpf_cutoff_hz",
        label: "MIT LPF cutoff (Hz)",
        description: "<= 0 disables one-pole smoothing on MIT targets.",
        category: "safety",
        unit: Some("Hz"),
        value_kind: "f32",
        min: Some(0.0),
        max: Some(100.0),
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "safety.mit_min_jerk_blend_ms",
        label: "MIT min-jerk blend (ms)",
        description: "0 disables minimum-jerk retiming between MIT targets.",
        category: "safety",
        unit: Some("ms"),
        value_kind: "f32",
        min: Some(0.0),
        max: Some(5000.0),
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RuntimeImmediate,
        tunable: true,
    },
    SettingDef {
        key: "telemetry.poll_interval_ms",
        label: "Telemetry poll interval (ms)",
        description: "Host poll cadence for type-17 / shadow registers.",
        category: "telemetry",
        unit: Some("ms"),
        value_kind: "u64",
        min: Some(1.0),
        max: None,
        requires_motors_stopped: true,
        apply_mode: WireApplyMode::RequiresRestart,
        tunable: true,
    },
];

pub fn def_by_key(key: &str) -> Option<&'static SettingDef> {
    ALL.iter().find(|d| d.key == key)
}

/// Extract the live JSON for one key from merged effective config.
pub fn value_from_merged(s: &SafetyConfig, t: &TelemetryConfig, key: &str) -> Option<Json> {
    let v = match key {
        "safety.require_verified" => json!(s.require_verified),
        "safety.boot_max_step_rad" => json!(s.boot_max_step_rad),
        "safety.step_size_rad" => json!(s.step_size_rad),
        "safety.tick_interval_ms" => json!(s.tick_interval_ms),
        "safety.homing_speed_rad_s" => match s.homing_speed_rad_s {
            None => Json::Null,
            Some(x) => json!(x),
        },
        "safety.tracking_error_max_rad" => json!(s.tracking_error_max_rad),
        "safety.tracking_error_grace_ticks" => json!(s.tracking_error_grace_ticks),
        "safety.tracking_freshness_max_age_ms" => json!(s.tracking_freshness_max_age_ms),
        "safety.tracking_error_debounce_ticks" => json!(s.tracking_error_debounce_ticks),
        "safety.band_violation_debounce_ticks" => json!(s.band_violation_debounce_ticks),
        "safety.boot_tracking_error_max_rad" => json!(s.boot_tracking_error_max_rad),
        "safety.target_tolerance_rad" => json!(s.target_tolerance_rad),
        "safety.target_dwell_ticks" => json!(s.target_dwell_ticks),
        "safety.target_dwell_max_vel_rad_s" => json!(s.target_dwell_max_vel_rad_s),
        "safety.homer_timeout_ms" => json!(s.homer_timeout_ms),
        "safety.max_feedback_age_ms" => json!(s.max_feedback_age_ms),
        "safety.commission_readback_tolerance_rad" => {
            json!(s.commission_readback_tolerance_rad)
        }
        "safety.auto_home_on_boot" => json!(s.auto_home_on_boot),
        "safety.scan_on_boot" => json!(s.scan_on_boot),
        "safety.hold_kp_nm_per_rad" => json!(s.hold_kp_nm_per_rad),
        "safety.hold_kd_nm_s_per_rad" => json!(s.hold_kd_nm_s_per_rad),
        "safety.motion_backend" => json!(s.motion_backend),
        "safety.mit_command_rate_hz" => json!(s.mit_command_rate_hz),
        "safety.mit_max_angle_step_rad" => json!(s.mit_max_angle_step_rad),
        "safety.mit_lpf_cutoff_hz" => json!(s.mit_lpf_cutoff_hz),
        "safety.mit_min_jerk_blend_ms" => json!(s.mit_min_jerk_blend_ms),
        "telemetry.poll_interval_ms" => json!(t.poll_interval_ms),
        _ => return None,
    };
    Some(v)
}

/// Seed (file) value from loaded [`Config`].
pub fn value_from_file_cfg(cfg: &Config, key: &str) -> Option<Json> {
    value_from_merged(&cfg.safety, &cfg.telemetry, key)
}

/// Build a map of seed key → JSON for diff / reset.
pub fn file_seed_map(cfg: &Config) -> std::collections::BTreeMap<String, Json> {
    file_defaults_to_kv(cfg).into_iter().collect()
}

/// True when the key is present in `settings_kv` (operator override or imported row).
pub fn is_key_in_db(kv: &std::collections::BTreeMap<String, String>, key: &str) -> bool {
    kv.contains_key(key)
}

/// `true` when the row in DB differs from the TOML seed (operator changed something).
pub fn is_dirty_merged(
    s: &SafetyConfig,
    t: &TelemetryConfig,
    cfg: &Config,
    key: &str,
    in_db: bool,
) -> bool {
    if !in_db {
        return false;
    }
    value_from_merged(s, t, key) != value_from_merged(&cfg.safety, &cfg.telemetry, key)
}

const PROFILE_PREFIX: &str = "profile:";

/// Stable meta key for a named profile.
pub fn profile_meta_key(name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty()
        || !n
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(format!("{PROFILE_PREFIX}{n}"))
}

pub fn profile_name_from_meta_key(k: &str) -> Option<&str> {
    k.strip_prefix(PROFILE_PREFIX)
}
