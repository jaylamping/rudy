//! Per-actuator soft travel-limits enforcement.
//!
//! Reads the live `Inventory` and rejects any commanded position that falls
//! outside the motor's `travel_limits` band. Reused by the jog endpoint
//! (today) and any future move-to / position-target endpoints (tomorrow).
//!
//! The hardware envelope (±4π per RS03 spec) is the absolute outer rail for
//! validation in `validate_band` — both the operator UI and the daemon
//! refuse to write a band wider than that.

use anyhow::Result;

use crate::inventory::TravelLimits;
use crate::state::SharedState;
use crate::types::SafetyEvent;

/// Outer rail used to bound every `travel_limits` write. Matches the RS03
/// MIT-mode position-control encoding (`op_control_scaling.position.range`
/// in `config/actuators/robstride_rs03.yaml`). A band wider than this would
/// be useless because the firmware can't even receive a setpoint outside
/// it.
pub const HARDWARE_POSITION_MIN_RAD: f32 = -4.0 * std::f32::consts::PI;
pub const HARDWARE_POSITION_MAX_RAD: f32 = 4.0 * std::f32::consts::PI;

/// Validate a candidate `[min_rad, max_rad]` band against the hardware outer
/// rail and basic monotonicity. Returns the static reason string the API
/// layer should surface verbatim to the SPA (or `Ok(())`).
pub fn validate_band(min_rad: f32, max_rad: f32) -> Result<(), &'static str> {
    if !min_rad.is_finite() || !max_rad.is_finite() {
        return Err("non-finite travel bound");
    }
    if min_rad >= max_rad {
        return Err("travel min must be strictly less than travel max");
    }
    if min_rad < HARDWARE_POSITION_MIN_RAD {
        return Err("travel min below hardware envelope");
    }
    if max_rad > HARDWARE_POSITION_MAX_RAD {
        return Err("travel max above hardware envelope");
    }
    Ok(())
}

/// Outcome of enforcing the band on a commanded position.
#[derive(Debug, Clone)]
pub enum BandCheck {
    /// No travel band on file → unrestricted (firmware envelope still applies).
    NoLimit,
    /// Inside the band; safe to forward.
    InBand { min_rad: f32, max_rad: f32 },
    /// Outside the band. The caller should reject the request and audit-log;
    /// `enforce_position` already broadcast the corresponding `SafetyEvent`.
    OutOfBand {
        min_rad: f32,
        max_rad: f32,
        attempted_rad: f32,
    },
}

/// Look up the travel band for `role` and check `target_rad` against it.
/// Broadcasts a `SafetyEvent::TravelLimitViolation` on rejection so the
/// dashboard can render the alert without polling.
///
/// Returns `Ok(BandCheck::OutOfBand)` rather than `Err` so handlers can
/// decide how to surface the rejection (e.g. as a 409 with structured
/// detail rather than a 500 with anyhow text).
pub fn enforce_position(state: &SharedState, role: &str, target_rad: f32) -> Result<BandCheck> {
    let limits: Option<TravelLimits> = state
        .inventory
        .read()
        .map_err(|_| anyhow::anyhow!("inventory poisoned"))?
        .by_role(role)
        .and_then(|m| m.travel_limits.clone());
    let Some(limits) = limits else {
        return Ok(BandCheck::NoLimit);
    };
    if target_rad < limits.min_rad || target_rad > limits.max_rad {
        let _ = state
            .safety_event_tx
            .send(SafetyEvent::TravelLimitViolation {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.to_string(),
                attempted_rad: target_rad,
                min_rad: limits.min_rad,
                max_rad: limits.max_rad,
            });
        return Ok(BandCheck::OutOfBand {
            min_rad: limits.min_rad,
            max_rad: limits.max_rad,
            attempted_rad: target_rad,
        });
    }
    Ok(BandCheck::InBand {
        min_rad: limits.min_rad,
        max_rad: limits.max_rad,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_band_rejects_inverted_band() {
        assert!(validate_band(1.0, -1.0).is_err());
        assert!(validate_band(0.0, 0.0).is_err());
    }

    #[test]
    fn validate_band_rejects_non_finite() {
        assert!(validate_band(f32::NAN, 1.0).is_err());
        assert!(validate_band(0.0, f32::INFINITY).is_err());
    }

    #[test]
    fn validate_band_enforces_outer_rail() {
        assert!(validate_band(HARDWARE_POSITION_MIN_RAD - 0.01, 0.0).is_err());
        assert!(validate_band(0.0, HARDWARE_POSITION_MAX_RAD + 0.01).is_err());
    }

    #[test]
    fn validate_band_accepts_normal_band() {
        assert!(validate_band(-1.0, 1.0).is_ok());
    }
}
