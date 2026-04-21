//! Wave pattern: symmetric oscillation around a fixed center.
//!
//! Mechanically identical to [`super::sweep`] except the
//! turnaround thresholds are `center ± amplitude` instead of the band
//! edges. Sharing the same shape lets the controller treat both
//! patterns through the same per-tick branch — only the
//! `MotionIntent` variant the controller pulls parameters from changes.

use crate::inventory::TravelLimits;

/// Per-run mutable state for a wave. Currently just a direction flag,
/// matching the sweep state — the wave pattern doesn't accumulate any
/// extra info between ticks. Kept as a struct so adding a phase counter
/// later is a one-field change with no signature churn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveState {
    pub direction: f32,
}

impl WaveState {
    pub fn from_position(pos_rad: f32, center_rad: f32) -> Self {
        Self {
            direction: if pos_rad <= center_rad { 1.0 } else { -1.0 },
        }
    }
}

/// Compute the velocity setpoint for the next tick of a wave. Pure
/// function; the controller pipes the live `mech_pos_rad` in and threads
/// the returned `WaveState` for the next call.
pub fn step(
    pos_rad: f32,
    state: WaveState,
    limits: &TravelLimits,
    center_rad: f32,
    amplitude_rad: f32,
    speed_rad_s: f32,
    turnaround_rad: f32,
) -> (f32, WaveState) {
    let speed = speed_rad_s.abs();
    let amp = amplitude_rad.abs();
    let turnaround = turnaround_rad.max(0.0);

    let center = center_rad.clamp(limits.min_rad, limits.max_rad);
    let raw_upper = (center + amp).min(limits.max_rad);
    let raw_lower = (center - amp).max(limits.min_rad);

    let upper = (raw_upper - turnaround).max(raw_lower);
    let lower = (raw_lower + turnaround).min(raw_upper);

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
    (vel, WaveState { direction })
}
