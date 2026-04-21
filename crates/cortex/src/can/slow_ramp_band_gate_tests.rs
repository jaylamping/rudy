//! Pins the band-violation debounce gate symmetric with the
//! tracking-error gate. Each test mirrors a tracking-error case so the
//! two gates stay in sync as we tune defaults.

use super::band_violation_should_abort;

#[test]
fn debounce_trips_on_third_consecutive_fresh_violation() {
    let mut c = 0;
    assert!(!band_violation_should_abort(
        true, true, 4, 3, true, 3, &mut c, "m"
    ));
    assert_eq!(c, 1);
    assert!(!band_violation_should_abort(
        true, true, 5, 3, true, 3, &mut c, "m"
    ));
    assert_eq!(c, 2);
    assert!(band_violation_should_abort(
        true, true, 6, 3, true, 3, &mut c, "m"
    ));
    assert_eq!(c, 3);
}

#[test]
fn single_in_band_sample_resets_band_debounce() {
    let mut c = 0;
    assert!(!band_violation_should_abort(
        true, true, 4, 3, true, 3, &mut c, "m"
    ));
    assert_eq!(c, 1);
    assert!(!band_violation_should_abort(
        true, true, 5, 3, false, 3, &mut c, "m"
    ));
    assert_eq!(c, 0);
    assert!(!band_violation_should_abort(
        true, true, 6, 3, true, 3, &mut c, "m"
    ));
    assert_eq!(c, 1);
}

#[test]
fn stale_tick_leaves_band_consec_unchanged() {
    let mut c = 2;
    assert!(!band_violation_should_abort(
        true, false, 10, 0, true, 3, &mut c, "m"
    ));
    assert_eq!(c, 2);
}

#[test]
fn mock_mode_skips_band_abort_in_gate() {
    // Note: the mock-mode immediate-abort path lives at the call site
    // (we pin the slow_ramp loop's mock-mode short-circuit
    // separately). The gate itself stays inert in mock mode so the
    // call site retains full control of the timing.
    let mut c = 0;
    assert!(!band_violation_should_abort(
        false, true, 100, 0, true, 3, &mut c, "m"
    ));
    assert_eq!(c, 0);
}

#[test]
fn grace_suppresses_band_abort() {
    let mut c = 0;
    assert!(!band_violation_should_abort(
        true, true, 2, 3, true, 1, &mut c, "m"
    ));
    assert_eq!(c, 0);
}
