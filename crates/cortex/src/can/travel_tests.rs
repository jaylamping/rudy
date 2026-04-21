use super::*;

fn rail_pm_four_pi() -> (f32, f32) {
    let w = 4.0 * std::f32::consts::PI;
    (-w, w)
}

#[test]
fn validate_band_rejects_inverted_band() {
    let (lo, hi) = rail_pm_four_pi();
    assert!(validate_band(1.0, -1.0, lo, hi).is_err());
    assert!(validate_band(0.0, 0.0, lo, hi).is_err());
}

#[test]
fn validate_band_rejects_non_finite() {
    let (lo, hi) = rail_pm_four_pi();
    assert!(validate_band(f32::NAN, 1.0, lo, hi).is_err());
    assert!(validate_band(0.0, f32::INFINITY, lo, hi).is_err());
}

#[test]
fn validate_band_enforces_outer_rail() {
    let (lo, hi) = rail_pm_four_pi();
    assert!(validate_band(lo - 0.01, 0.0, lo, hi).is_err());
    assert!(validate_band(0.0, hi + 0.01, lo, hi).is_err());
}

#[test]
fn validate_band_accepts_normal_band() {
    let (lo, hi) = rail_pm_four_pi();
    assert!(validate_band(-1.0, 1.0, lo, hi).is_ok());
}

/// Wider vs narrow MIT rails behave like distinct actuator models.
#[test]
fn travel_rail_from_spec_per_model() {
    let narrow = (-1.0_f32, 1.0_f32);
    assert!(validate_band(-0.5, 0.5, narrow.0, narrow.1).is_ok());
    assert!(validate_band(-2.0, 0.0, narrow.0, narrow.1).is_err());

    let wide = (-10.0_f32, 10.0_f32);
    assert!(validate_band(-2.0, 0.0, wide.0, wide.1).is_ok());
}
