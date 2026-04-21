//! Sweep pattern: drive end-to-end across the configured travel band,
//! reversing direction just shy of each edge.
//!
//! The step function in here is pure (current position + state →
//! next velocity + next state) so it's trivial to unit-test without a
//! tokio runtime, a CAN bus, or even an `AppState`. The
//! [`crate::motion::controller`] is the only consumer; it pipes
//! `state.latest[role].mech_pos_rad` in on every tick and drops the
//! returned velocity onto the bus worker.

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

    let mut direction = state.direction;
    if direction > 0.0 && pos_rad >= upper {
        direction = -1.0;
    } else if direction < 0.0 && pos_rad <= lower {
        direction = 1.0;
    }

    let vel = if upper <= lower {
        0.0
    } else {
        direction * speed
    };
    (vel, SweepState { direction })
}
