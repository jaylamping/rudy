//! Whole-snapshot validation (single-key range checks are not enough).

use crate::config::{SafetyConfig, TelemetryConfig};

pub fn validate_snapshot(s: &SafetyConfig, t: &TelemetryConfig) -> Result<(), String> {
    if s.tick_interval_ms < 1 {
        return Err("safety.tick_interval_ms must be >= 1".to_string());
    }
    if s.target_tolerance_rad < s.step_size_rad * 0.5 {
        return Err("safety.target_tolerance_rad should be at least 0.5 * step_size_rad to avoid home-ramp bounce".to_string());
    }
    if s.homer_timeout_ms < 1_000 {
        return Err("safety.homer_timeout_ms unreasonably small".to_string());
    }
    if t.poll_interval_ms < 1 {
        return Err("telemetry.poll_interval_ms must be >= 1".to_string());
    }
    // Worst-case idle fallback gap ≈ N * poll; max_feedback should exceed that
    // on multi-motor bus — keep a soft check without knowing N.
    if s.max_feedback_age_ms < t.poll_interval_ms * 2 {
        return Err(
            "safety.max_feedback_age_ms should be >= 2x telemetry.poll_interval_ms".to_string(),
        );
    }
    if !s.target_dwell_max_vel_rad_s.is_finite() {
        // ok: f32::INFINITY disables vel gate
    } else if s.target_dwell_max_vel_rad_s < 0.0 {
        return Err("safety.target_dwell_max_vel_rad_s must be non-negative or +inf".to_string());
    }
    Ok(())
}

/// Apply post-merge recovery override: no motion while operator has not ack'd re-seed.
pub fn apply_recovery(safety: &mut SafetyConfig, recovery_pending: bool) {
    if recovery_pending {
        safety.auto_home_on_boot = false;
    }
}
