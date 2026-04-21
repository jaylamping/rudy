//! Sweep pattern: drive end-to-end across the configured travel band,
//! reversing direction just shy of each edge.
//!
//! The step function in here is pure (current position + state →
//! next velocity + next state) so it's trivial to unit-test without a
//! tokio runtime, a CAN bus, or even an `AppState`. The
//! [`crate::motion::controller`] is the only consumer; it pipes
//! `state.latest[role].mech_pos_rad` in on every tick and drops the
//! returned velocity onto the bus_worker.

use crate::inventory::TravelLimits;

/// Per-run mutable state for a sweep. Initialised once at controller
/// start; updated in place by [`step`] on every tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepState {
    /// `+1.0` means "moving toward `max_rad`," `-1.0` means "moving
    /// toward `min_rad`." Initialised based on which end of the band the
    /// motor is closer to so the very first frame heads "outward."
    pub direction: f32,
}

impl SweepState {
    /// Pick the initial direction so the motor heads toward the edge it
    /// is *farther* from. Avoids the visually-weird "start by moving
    /// 1 cm toward the near edge" jolt when the motor is bunched up
    /// near a band end at start time.
    pub fn from_position(pos_rad: f32, limits: &TravelLimits) -> Self {
        let mid = 0.5 * (limits.min_rad + limits.max_rad);
        let direction = if pos_rad <= mid { 1.0 } else { -1.0 };
        Self { direction }
    }
}

/// Compute the velocity setpoint for the next tick. Pure function; the
/// caller owns time, IO, and the bus.
///
/// Returns `(next_velocity_rad_s, next_state)`. `next_state` may have a
/// flipped `direction` if the motor crossed the turnaround threshold;
/// the controller swaps it back into its own state.
pub fn step(
    pos_rad: f32,
    state: SweepState,
    limits: &TravelLimits,
    speed_rad_s: f32,
    turnaround_rad: f32,
) -> (f32, SweepState) {
    let speed = speed_rad_s.abs();
    let turnaround = turnaround_rad.max(0.0);

    let upper = (limits.max_rad - turnaround).max(limits.min_rad);
    let lower = (limits.min_rad + turnaround).min(limits.max_rad);

    // Flip when we've crossed (or are at) the inset on the side we're
    // currently heading toward. The check uses position only, never time,
    // so a frozen telemetry row can't fool the controller into thinking
    // it has reached the edge when it hasn't.
    let mut direction = state.direction;
    if direction > 0.0 && pos_rad >= upper {
        direction = -1.0;
    } else if direction < 0.0 && pos_rad <= lower {
        direction = 1.0;
    }

    // Defensive: if the band collapsed to a point (lower >= upper after
    // turnaround inset), command zero so we don't oscillate at full
    // speed inside a 1 mm window.
    let vel = if upper <= lower {
        0.0
    } else {
        direction * speed
    };
    (vel, SweepState { direction })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(min: f32, max: f32) -> TravelLimits {
        TravelLimits {
            min_rad: min,
            max_rad: max,
            updated_at: None,
        }
    }

    #[test]
    fn initial_direction_from_band_midpoint() {
        let l = limits(-1.0, 1.0);
        // At the lower half of the band → move toward max.
        assert_eq!(SweepState::from_position(-0.5, &l).direction, 1.0);
        // At the upper half → move toward min.
        assert_eq!(SweepState::from_position(0.5, &l).direction, -1.0);
        // Exactly at the midpoint: deterministic ("toward max" wins so
        // the test doesn't care which way; we just pin the convention).
        assert_eq!(SweepState::from_position(0.0, &l).direction, 1.0);
    }

    #[test]
    fn step_flips_direction_at_inset() {
        let l = limits(-1.0, 1.0);
        let s = SweepState { direction: 1.0 };
        // Just inside the turnaround inset → keep going.
        let (v, ns) = step(0.5, s, &l, 0.1, 0.05);
        assert!(v > 0.0);
        assert_eq!(ns.direction, 1.0);
        // Past the inset on the upper side → flip and head down.
        let (v, ns) = step(0.96, s, &l, 0.1, 0.05);
        assert!(v < 0.0);
        assert_eq!(ns.direction, -1.0);
    }

    #[test]
    fn step_flips_direction_at_lower_inset() {
        let l = limits(-1.0, 1.0);
        let s = SweepState { direction: -1.0 };
        let (v, ns) = step(-0.96, s, &l, 0.1, 0.05);
        assert!(v > 0.0);
        assert_eq!(ns.direction, 1.0);
    }

    #[test]
    fn step_speed_magnitude_is_caller_supplied() {
        let l = limits(-1.0, 1.0);
        let s = SweepState { direction: 1.0 };
        let (v, _) = step(0.0, s, &l, 0.42, 0.05);
        assert!((v.abs() - 0.42).abs() < 1e-6);
    }

    #[test]
    fn step_collapsed_band_returns_zero_velocity() {
        // Turnaround inset wider than half the band → controller refuses
        // to move rather than oscillating in a tiny window.
        let l = limits(-0.05, 0.05);
        let s = SweepState { direction: 1.0 };
        let (v, _) = step(0.0, s, &l, 0.1, 0.5);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn step_negative_speed_treated_as_magnitude() {
        // Caller passes magnitude; sign always comes from `direction`.
        // A negative `speed_rad_s` must NOT silently reverse direction
        // (that would smuggle direction control out of the controller).
        let l = limits(-1.0, 1.0);
        let s = SweepState { direction: 1.0 };
        let (v, _) = step(0.0, s, &l, -0.3, 0.05);
        assert!(v > 0.0);
        assert!((v - 0.3).abs() < 1e-6);
    }
}
