//! Pins the predictive band-edge velocity-cap helper. The integration
//! behavior (motor that would otherwise overshoot now decelerates
//! before the edge) is exercised end-to-end by
//! `slow_ramp_real_can_stub_tests`; these unit tests focus on the
//! math.

use super::band_edge_distance;
use crate::inventory::TravelLimits;

fn limits(min_rad: f32, max_rad: f32) -> TravelLimits {
    TravelLimits {
        min_rad,
        max_rad,
        updated_at: None,
    }
}

#[test]
fn no_limits_returns_infinity() {
    // Without travel_limits the cap is a no-op and the slow-ramp
    // falls back to the original `governing` taper. Encoded as
    // f32::INFINITY so the caller's `min(|governing|, _)` lands on
    // `|governing|` unchanged.
    let d = band_edge_distance(None, 0.0, 1.0);
    assert!(d.is_infinite());
}

#[test]
fn zero_direction_returns_infinity() {
    // Direction == 0 means the slow-ramp already commanded `vel = 0`
    // (we're parked at the target); no edge to fear.
    let l = limits(-1.0, 1.0);
    let d = band_edge_distance(Some(&l), 0.5, 0.0);
    assert!(d.is_infinite());
}

#[test]
fn positive_direction_yields_distance_to_max() {
    let l = limits(-1.0, 1.0);
    let d = band_edge_distance(Some(&l), 0.7, 1.0);
    assert!((d - 0.3).abs() < 1e-6);
}

#[test]
fn negative_direction_yields_distance_to_min() {
    let l = limits(-1.0, 1.0);
    let d = band_edge_distance(Some(&l), -0.7, -1.0);
    assert!((d - 0.3).abs() < 1e-6);
}

#[test]
fn measured_already_past_edge_clamps_to_zero() {
    // If `last_measured` has already crossed the band edge in the
    // direction of motion (the band check will abort separately), the
    // cap returns 0 so the velocity command goes to zero on this tick
    // — we never want the homer pushing further into the overshoot
    // while the band-debounce is making up its mind.
    let l = limits(-1.0, 1.0);
    let d = band_edge_distance(Some(&l), 1.1, 1.0);
    assert_eq!(d, 0.0);
}

#[test]
fn principal_angle_wrap_is_respected() {
    // `last_measured = 6.4 rad` wraps to ~+0.117 rad, well inside a
    // [-1.0, +1.0] band. Distance to +1.0 in the positive direction
    // is therefore ~0.883 rad, NOT a negative number from the
    // unwrapped 6.4. This matches the convention
    // `enforce_position_with_path` uses.
    let l = limits(-1.0, 1.0);
    let d = band_edge_distance(Some(&l), 6.4, 1.0);
    let principal = 6.4 - 2.0 * std::f32::consts::PI;
    let expected = 1.0 - principal;
    assert!((d - expected).abs() < 1e-5, "d={d} expected={expected}");
}
