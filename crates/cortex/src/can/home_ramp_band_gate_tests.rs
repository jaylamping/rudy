//! Pins the band-violation gate. Travel-band violations are fail-closed:
//! first fresh out-of-band sample aborts before another velocity frame.

use super::band_violation_should_abort;

#[test]
fn aborts_on_first_fresh_violation() {
    let mut c = 0;
    assert!(band_violation_should_abort(true, true, true, &mut c, "m"));
    assert_eq!(c, 1);
}

#[test]
fn single_in_band_sample_resets_band_counter() {
    let mut c = 0;
    assert!(band_violation_should_abort(true, true, true, &mut c, "m"));
    assert_eq!(c, 1);
    assert!(!band_violation_should_abort(true, true, false, &mut c, "m"));
    assert_eq!(c, 0);
}

#[test]
fn stale_tick_leaves_band_counter_unchanged() {
    let mut c = 2;
    assert!(!band_violation_should_abort(true, false, true, &mut c, "m"));
    assert_eq!(c, 2);
}

#[test]
fn mock_mode_skips_band_abort_in_gate() {
    // Note: the mock-mode immediate-abort path lives at the call site
    // (we pin the home_ramp loop's mock-mode short-circuit
    // separately). The gate itself stays inert in mock mode so the
    // call site retains full control of the timing.
    let mut c = 0;
    assert!(!band_violation_should_abort(false, true, true, &mut c, "m"));
    assert_eq!(c, 0);
}

#[test]
fn no_grace_window_for_fresh_band_violation() {
    let mut c = 0;
    assert!(band_violation_should_abort(true, true, true, &mut c, "m"));
    assert_eq!(c, 1);
}
