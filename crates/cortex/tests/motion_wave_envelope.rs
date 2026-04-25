//! Characterization: wave pattern step uses explicit `TravelLimits` band
//! (same ±π envelope operators configure for unbounded wave tests).

#[path = "common/mod.rs"]
mod common;

use cortex::inventory::TravelLimits;
use cortex::motion::patterns::wave::{step, WaveState};
use std::f32::consts::PI;

#[test]
fn wave_step_nonzero_velocity_in_pi_envelope() {
    let limits = TravelLimits {
        min_rad: -PI,
        max_rad: PI,
        updated_at: None,
    };
    let state = WaveState::from_position(0.0, 0.0);
    let (v, _) = step(0.0, state, &limits, 0.0, 0.5, 0.1, 0.02);
    assert!(
        v.abs() > 0.0,
        "expected non-zero wave velocity in ±π envelope"
    );
}
