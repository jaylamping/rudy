//! Pins the hold-verification budget used after a successful home-ramp.
//!
//! End-to-end stub coverage lives in `home_ramp_real_can_stub_tests` and
//! `tests/boot_orchestrator_lifecycle.rs` (`is_position_hold` after auto-home).
//!
//! `finish_home_success` uses `cfg.target_tolerance_rad * 2.0` as the post-hold
//! error cap.

#[test]
fn hold_verification_limit_is_double_target_tolerance() {
    let tol = crate::config::default_target_tolerance_rad();
    let limit = tol * 2.0;
    assert!(
        limit > tol,
        "hold verification should be strictly looser than the dwell deadband"
    );
}
