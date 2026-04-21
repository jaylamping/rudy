use super::*;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

#[test]
fn wrap_to_pi_handles_principal_range() {
    for v in [-PI + 1e-3, -1.0, 0.0, 1.0, PI - 1e-3] {
        assert!((wrap_to_pi(v) - v).abs() < 1e-5, "{v}");
    }
}

#[test]
fn wrap_to_pi_collapses_multi_revolution() {
    // 3*pi -> +pi (or -pi; we accept either since they're the same point)
    let r = wrap_to_pi(3.0 * PI);
    assert!((r - PI).abs() < 1e-4 || (r + PI).abs() < 1e-4, "got {r}");
    let r = wrap_to_pi(-3.0 * PI);
    assert!((r - PI).abs() < 1e-4 || (r + PI).abs() < 1e-4, "got {r}");

    // 359 deg = -1 deg; canonical form is -1 deg.
    let r = wrap_to_pi(359.0_f32.to_radians());
    assert!(
        (r - (-1.0_f32).to_radians()).abs() < 1e-4,
        "got {} deg",
        r.to_degrees()
    );
}

#[test]
fn shortest_signed_delta_picks_shorter() {
    // The disaster-scenario test. current=+170 deg, target=-170 deg.
    // The naive (target - current) is -340 deg -> motor takes the long
    // way and rips out wiring. The principal-angle delta is +20 deg.
    let cur = 170.0_f32.to_radians();
    let tgt = (-170.0_f32).to_radians();
    let d = shortest_signed_delta(cur, tgt);
    assert!(
        (d - 20.0_f32.to_radians()).abs() < 1e-4,
        "expected +20 deg, got {} deg",
        d.to_degrees()
    );
    assert!(d.abs() < PI, "shortest delta must be in [-pi,+pi]");
}

#[test]
fn shortest_signed_delta_at_pi_boundary() {
    // current=0, target=+pi. Delta is +/-pi (both equally short); we
    // accept either deterministic answer as long as |delta| == pi.
    let d = shortest_signed_delta(0.0, PI);
    assert!((d.abs() - PI).abs() < 1e-4, "got {d}");
}

#[test]
fn shortest_signed_delta_zero_when_equal() {
    for v in [-FRAC_PI_2, 0.0, FRAC_PI_2, 1.234] {
        let d = shortest_signed_delta(v, v);
        assert!(d.abs() < 1e-5, "{v} -> {d}");
    }
}

#[test]
fn wrap_to_pi_saturates_nan_and_inf() {
    assert_eq!(wrap_to_pi(f32::NAN), 0.0);
    assert_eq!(wrap_to_pi(f32::INFINITY), 0.0);
    assert_eq!(wrap_to_pi(f32::NEG_INFINITY), 0.0);
}

#[test]
fn shortest_signed_delta_collapses_multi_revolution_inputs() {
    // current = +1 deg + 2 full revs; target = -1 deg. The wrapped
    // current is +1 deg, so the shortest delta is -2 deg.
    let cur = 1.0_f32.to_radians() + 2.0 * TAU;
    let tgt = (-1.0_f32).to_radians();
    let d = shortest_signed_delta(cur, tgt);
    assert!(
        (d - (-2.0_f32).to_radians()).abs() < 1e-4,
        "expected -2 deg, got {} deg",
        d.to_degrees()
    );
}
