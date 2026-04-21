//! Per-actuator soft travel-limits enforcement.
//!
//! Reads the live `Inventory` and rejects any commanded position that falls
//! outside the motor's `travel_limits` band. Reused by the jog endpoint
//! (today) and any future move-to / position-target endpoints (tomorrow).
//!
//! The hardware envelope for travel-limit writes is the MIT position rail from
//! each actuator's loaded spec ([`crate::spec::ActuatorSpec::mit_position_rail_rad`]
//! — `op_control_scaling.position.range` in `robstride_*.yaml`).

use anyhow::Result;

use crate::can::angle::UnwrappedAngle;
use crate::can::motion::{shortest_signed_delta, wrap_to_pi};
use crate::inventory::TravelLimits;
use crate::state::SharedState;
use crate::types::SafetyEvent;

/// Validate a candidate `[min_rad, max_rad]` band against the MIT position outer
/// rail for this actuator model and basic monotonicity. Returns the static reason
/// string the API layer should surface verbatim to the SPA (or `Ok(())`).
///
/// `hardware_position_*` come from [`crate::spec::ActuatorSpec::mit_position_rail_rad`].
pub fn validate_band(
    min_rad: f32,
    max_rad: f32,
    hardware_position_min_rad: f32,
    hardware_position_max_rad: f32,
) -> Result<(), &'static str> {
    if !min_rad.is_finite() || !max_rad.is_finite() {
        return Err("non-finite travel bound");
    }
    if min_rad >= max_rad {
        return Err("travel min must be strictly less than travel max");
    }
    if min_rad < hardware_position_min_rad {
        return Err("travel min below hardware envelope");
    }
    if max_rad > hardware_position_max_rad {
        return Err("travel max above hardware envelope");
    }
    Ok(())
}

/// Outcome of enforcing the band on a commanded position.
#[derive(Debug, Clone)]
pub enum BandCheck {
    /// No travel band on file → unrestricted (firmware envelope still applies).
    NoLimit,
    /// Inside the band; safe to forward. `delta_rad` is the shortest signed
    /// principal-angle distance from current to target, set by
    /// `enforce_position_with_path`.
    InBand {
        min_rad: f32,
        max_rad: f32,
        delta_rad: f32,
    },
    /// Target endpoint is outside the band. The caller should reject the
    /// request and audit-log; the enforcer already broadcast the
    /// corresponding `SafetyEvent`.
    OutOfBand {
        min_rad: f32,
        max_rad: f32,
        attempted_rad: f32,
    },
    /// Target endpoint is inside the band but the swept arc crosses the
    /// band boundary (current position is outside). Rejected by motion
    /// endpoints; the home-ramp homer refuses this shape as well.
    PathViolation {
        min_rad: f32,
        max_rad: f32,
        current_rad: f32,
        target_rad: f32,
    },
}

/// Principal-angle path-aware band check. Use this from any handler that
/// produces motion (jog, home, bench-tests-that-command-position).
///
/// Both `current_rad` and `target_rad` are reduced to principal angles in
/// [-pi, +pi] before the check. The check passes only when:
///
///  1. the principal target endpoint is inside `[min_rad, max_rad]`, AND
///  2. the principal current position is also inside the band — which, for
///     the < 360 deg cable-bound joints we have, guarantees the swept
///     shortest-path arc stays in band.
///
/// If only condition 1 holds (target in band but current outside), the
/// result is [`BandCheck::PathViolation`] — the swept arc would cross the
/// boundary. This is the chokepoint that prevents the multi-turn-encoder
/// disaster: the firmware might still take a long path, but a daemon that
/// commands "go to 0 deg" while reading "+20 deg (actually +20 deg + 360)"
/// is refused before any frame leaves the host.
pub fn enforce_position_with_path(
    state: &SharedState,
    role: &str,
    current: UnwrappedAngle,
    target: UnwrappedAngle,
) -> Result<BandCheck> {
    let limits: Option<TravelLimits> = state
        .inventory
        .read()
        .map_err(|_| anyhow::anyhow!("inventory poisoned"))?
        .actuator_by_role(role)
        .and_then(|m| m.common.travel_limits.clone());
    let Some(limits) = limits else {
        return Ok(BandCheck::NoLimit);
    };

    let cur_p = wrap_to_pi(current.raw());
    let tgt_p = wrap_to_pi(target.raw());

    if tgt_p < limits.min_rad || tgt_p > limits.max_rad {
        let _ = state
            .safety_event_tx
            .send(SafetyEvent::TravelLimitViolation {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.to_string(),
                attempted_rad: tgt_p,
                min_rad: limits.min_rad,
                max_rad: limits.max_rad,
            });
        return Ok(BandCheck::OutOfBand {
            min_rad: limits.min_rad,
            max_rad: limits.max_rad,
            attempted_rad: tgt_p,
        });
    }

    if cur_p < limits.min_rad || cur_p > limits.max_rad {
        let _ = state
            .safety_event_tx
            .send(SafetyEvent::TravelLimitViolation {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.to_string(),
                attempted_rad: cur_p,
                min_rad: limits.min_rad,
                max_rad: limits.max_rad,
            });
        return Ok(BandCheck::PathViolation {
            min_rad: limits.min_rad,
            max_rad: limits.max_rad,
            current_rad: cur_p,
            target_rad: tgt_p,
        });
    }

    Ok(BandCheck::InBand {
        min_rad: limits.min_rad,
        max_rad: limits.max_rad,
        delta_rad: shortest_signed_delta(cur_p, tgt_p),
    })
}

#[cfg(test)]
#[path = "travel_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "travel_path_check_tests.rs"]
mod path_check_tests;
