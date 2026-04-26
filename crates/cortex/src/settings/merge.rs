//! Merge file `cortex.toml` with SQLite settings rows.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::json;
use serde_json::Value as Json;

use crate::config::{Config, MotionBackend, SafetyConfig, TelemetryConfig};

/// Seed rows from a loaded TOML config (idempotent: same values the DB was meant to have on first import).
pub fn file_defaults_to_kv(cfg: &Config) -> Vec<(String, Json)> {
    let s = &cfg.safety;
    let t = &cfg.telemetry;
    vec![
        ("safety.require_verified".into(), json!(s.require_verified)),
        (
            "safety.boot_max_step_rad".into(),
            json!(s.boot_max_step_rad),
        ),
        ("safety.step_size_rad".into(), json!(s.step_size_rad)),
        ("safety.tick_interval_ms".into(), json!(s.tick_interval_ms)),
        (
            "safety.homing_speed_rad_s".into(),
            match s.homing_speed_rad_s {
                None => Json::Null,
                Some(x) => json!(x),
            },
        ),
        (
            "safety.tracking_error_max_rad".into(),
            json!(s.tracking_error_max_rad),
        ),
        (
            "safety.tracking_error_grace_ticks".into(),
            json!(s.tracking_error_grace_ticks),
        ),
        (
            "safety.tracking_freshness_max_age_ms".into(),
            json!(s.tracking_freshness_max_age_ms),
        ),
        (
            "safety.tracking_error_debounce_ticks".into(),
            json!(s.tracking_error_debounce_ticks),
        ),
        ("safety.fatal_warn_mask".into(), json!(s.fatal_warn_mask)),
        ("safety.fatal_fault_mask".into(), json!(s.fatal_fault_mask)),
        (
            "safety.band_violation_debounce_ticks".into(),
            json!(s.band_violation_debounce_ticks),
        ),
        (
            "safety.boot_tracking_error_max_rad".into(),
            json!(s.boot_tracking_error_max_rad),
        ),
        (
            "safety.target_tolerance_rad".into(),
            json!(s.target_tolerance_rad),
        ),
        (
            "safety.target_dwell_ticks".into(),
            json!(s.target_dwell_ticks),
        ),
        (
            "safety.target_dwell_max_vel_rad_s".into(),
            json!(s.target_dwell_max_vel_rad_s),
        ),
        ("safety.homer_timeout_ms".into(), json!(s.homer_timeout_ms)),
        (
            "safety.max_feedback_age_ms".into(),
            json!(s.max_feedback_age_ms),
        ),
        (
            "safety.commission_readback_tolerance_rad".into(),
            json!(s.commission_readback_tolerance_rad),
        ),
        (
            "safety.auto_home_on_boot".into(),
            json!(s.auto_home_on_boot),
        ),
        ("safety.scan_on_boot".into(), json!(s.scan_on_boot)),
        (
            "safety.hold_kp_nm_per_rad".into(),
            json!(s.hold_kp_nm_per_rad),
        ),
        (
            "safety.hold_kd_nm_s_per_rad".into(),
            json!(s.hold_kd_nm_s_per_rad),
        ),
        ("safety.motion_backend".into(), json!(s.motion_backend)),
        (
            "safety.mit_command_rate_hz".into(),
            json!(s.mit_command_rate_hz),
        ),
        (
            "safety.mit_max_angle_step_rad".into(),
            json!(s.mit_max_angle_step_rad),
        ),
        (
            "safety.mit_lpf_cutoff_hz".into(),
            json!(s.mit_lpf_cutoff_hz),
        ),
        (
            "safety.mit_min_jerk_blend_ms".into(),
            json!(s.mit_min_jerk_blend_ms),
        ),
        (
            "telemetry.poll_interval_ms".into(),
            json!(t.poll_interval_ms),
        ),
    ]
}

/// Parse loaded KV rows and merge onto file defaults (TOML `cfg` as baseline).
pub fn merge_from_kv(
    file_cfg: &Config,
    kv: BTreeMap<String, String>,
) -> Result<(SafetyConfig, TelemetryConfig)> {
    let mut s = file_cfg.safety.clone();
    let mut t = file_cfg.telemetry.clone();
    for (k, json_str) in kv {
        let v: Json = serde_json::from_str(&json_str).map_err(|e| anyhow::anyhow!("{k}: {e}"))?;
        apply_key(&mut s, &mut t, &k, v).map_err(|e| anyhow::anyhow!("{k}: {e}"))?;
    }
    Ok((s, t))
}

fn f32v(v: &Json) -> Option<f32> {
    v.as_f64().map(|x| x as f32)
}

/// Apply a single value; `k` is `safety.*` or `telemetry.*`.
pub fn apply_key_from_json(
    s: &mut SafetyConfig,
    t: &mut TelemetryConfig,
    k: &str,
    v: Json,
) -> std::result::Result<(), String> {
    apply_key(s, t, k, v)
}

fn apply_key(
    s: &mut SafetyConfig,
    t: &mut TelemetryConfig,
    k: &str,
    v: Json,
) -> std::result::Result<(), String> {
    match k {
        "safety.require_verified" => {
            s.require_verified = v.as_bool().ok_or_else(|| "expected bool".to_string())?
        }
        "safety.boot_max_step_rad" => s.boot_max_step_rad = f32v(&v).ok_or("expected f32")?,
        "safety.step_size_rad" => s.step_size_rad = f32v(&v).ok_or("expected f32")?,
        "safety.tick_interval_ms" => s.tick_interval_ms = v.as_u64().ok_or("expected u32")? as u32,
        "safety.homing_speed_rad_s" => {
            s.homing_speed_rad_s = if v.is_null() {
                None
            } else {
                Some(f32v(&v).ok_or("expected f32 or null")?)
            }
        }
        "safety.tracking_error_max_rad" => {
            s.tracking_error_max_rad = f32v(&v).ok_or("expected f32")?
        }
        "safety.tracking_error_grace_ticks" => {
            s.tracking_error_grace_ticks = v.as_u64().ok_or("expected u32")? as u32
        }
        "safety.tracking_freshness_max_age_ms" => {
            s.tracking_freshness_max_age_ms = v.as_u64().ok_or("expected u64")?
        }
        "safety.tracking_error_debounce_ticks" => {
            s.tracking_error_debounce_ticks = v.as_u64().ok_or("expected u32")? as u32
        }
        "safety.fatal_warn_mask" => s.fatal_warn_mask = v.as_u64().ok_or("expected u32")? as u32,
        "safety.fatal_fault_mask" => s.fatal_fault_mask = v.as_u64().ok_or("expected u32")? as u32,
        "safety.band_violation_debounce_ticks" => {
            s.band_violation_debounce_ticks = v.as_u64().ok_or("expected u32")? as u32
        }
        "safety.boot_tracking_error_max_rad" => {
            s.boot_tracking_error_max_rad = f32v(&v).ok_or("expected f32")?
        }
        "safety.target_tolerance_rad" => s.target_tolerance_rad = f32v(&v).ok_or("expected f32")?,
        "safety.target_dwell_ticks" => {
            s.target_dwell_ticks = v.as_u64().ok_or("expected u32")? as u32
        }
        "safety.target_dwell_max_vel_rad_s" => {
            s.target_dwell_max_vel_rad_s = f32v(&v).ok_or("expected f32")?
        }
        "safety.homer_timeout_ms" => s.homer_timeout_ms = v.as_u64().ok_or("expected u32")? as u32,
        "safety.max_feedback_age_ms" => s.max_feedback_age_ms = v.as_u64().ok_or("expected u64")?,
        "safety.commission_readback_tolerance_rad" => {
            s.commission_readback_tolerance_rad = f32v(&v).ok_or("expected f32")?
        }
        "safety.auto_home_on_boot" => s.auto_home_on_boot = v.as_bool().ok_or("expected bool")?,
        "safety.scan_on_boot" => s.scan_on_boot = v.as_bool().ok_or("expected bool")?,
        "safety.hold_kp_nm_per_rad" => s.hold_kp_nm_per_rad = f32v(&v).ok_or("expected f32")?,
        "safety.hold_kd_nm_s_per_rad" => s.hold_kd_nm_s_per_rad = f32v(&v).ok_or("expected f32")?,
        "safety.motion_backend" => {
            s.motion_backend = match v
                .as_str()
                .ok_or_else(|| "expected \"mit\" or \"velocity\" string".to_string())?
            {
                "mit" => MotionBackend::Mit,
                "velocity" => MotionBackend::Velocity,
                other => {
                    return Err(format!(
                        "unknown motion_backend {other:?} (expected mit|velocity)"
                    ))
                }
            };
        }
        "safety.mit_command_rate_hz" => {
            s.mit_command_rate_hz = f32v(&v).ok_or("expected f32")?;
        }
        "safety.mit_max_angle_step_rad" => {
            s.mit_max_angle_step_rad = f32v(&v).ok_or("expected f32")?;
        }
        "safety.mit_lpf_cutoff_hz" => {
            s.mit_lpf_cutoff_hz = f32v(&v).ok_or("expected f32")?;
        }
        "safety.mit_min_jerk_blend_ms" => {
            s.mit_min_jerk_blend_ms = f32v(&v).ok_or("expected f32")?;
        }
        "telemetry.poll_interval_ms" => t.poll_interval_ms = v.as_u64().ok_or("expected u64")?,

        _ => return Err("unknown key".to_string()),
    }
    Ok(())
}
