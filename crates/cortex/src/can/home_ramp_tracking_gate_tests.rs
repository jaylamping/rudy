//! Pins the one-sided tracking-error abort gate. Each test exercises
//! a single decision branch of `tracking_error_should_abort`; the
//! end-to-end behavior (including the `err_rad` lag projection done by
//! the home-ramp loop *before* it calls this gate) is exercised by
//! `home_ramp_real_can_stub_tests`.

use super::tracking_error_should_abort;

#[test]
fn debounce_trips_on_third_consecutive_fresh_over_budget() {
    let mut c = 0;
    assert!(!tracking_error_should_abort(
        true, true, 4, 3, 0.06, 0.05, 3, &mut c, "m"
    ));
    assert_eq!(c, 1);
    assert!(!tracking_error_should_abort(
        true, true, 5, 3, 0.06, 0.05, 3, &mut c, "m"
    ));
    assert_eq!(c, 2);
    assert!(tracking_error_should_abort(
        true, true, 6, 3, 0.06, 0.05, 3, &mut c, "m"
    ));
    assert_eq!(c, 3);
}

#[test]
fn single_good_sample_resets_debounce() {
    let mut c = 0;
    assert!(!tracking_error_should_abort(
        true, true, 4, 3, 0.06, 0.05, 3, &mut c, "m"
    ));
    assert_eq!(c, 1);
    assert!(!tracking_error_should_abort(
        true, true, 5, 3, 0.01, 0.05, 3, &mut c, "m"
    ));
    assert_eq!(c, 0);
    assert!(!tracking_error_should_abort(
        true, true, 6, 3, 0.06, 0.05, 3, &mut c, "m"
    ));
    assert_eq!(c, 1);
}

#[test]
fn stale_tick_leaves_consec_unchanged() {
    let mut c = 2;
    assert!(!tracking_error_should_abort(
        true, false, 10, 0, 1.0, 0.05, 3, &mut c, "m"
    ));
    assert_eq!(c, 2);
}

#[test]
fn mock_mode_skips_tracking_abort() {
    let mut c = 0;
    assert!(!tracking_error_should_abort(
        false, true, 100, 0, 10.0, 0.05, 3, &mut c, "m"
    ));
    assert_eq!(c, 0);
}

#[test]
fn grace_suppresses_tracking_abort() {
    let mut c = 0;
    assert!(!tracking_error_should_abort(
        true, true, 2, 3, 10.0, 0.05, 1, &mut c, "m"
    ));
    assert_eq!(c, 0);
}
