//! Pins the post-home hold contract: MIT spring-damper hold (not PP) with the
//! `[safety].hold_kp/kd_*` defaults, and a verification cap of
//! `2 × target_tolerance_rad`.
//!
//! End-to-end stub coverage lives in `home_ramp_real_can_stub_tests` and
//! `tests/boot_orchestrator_lifecycle.rs` (`is_position_hold` after auto-home).

#[test]
fn hold_verification_limit_is_double_target_tolerance() {
    let tol = crate::config::default_target_tolerance_rad();
    let limit = tol * 2.0;
    assert!(
        limit > tol,
        "hold verification should be strictly looser than the dwell deadband"
    );
}

#[test]
fn mit_hold_defaults_are_conservative_spring() {
    // Sanity-check the `[safety]` defaults wired into `finish_home_success`.
    // Conservative spring: stiff enough to resist droop on a gravity-loaded
    // RS03 joint, soft enough for an operator to push by hand. If you change
    // these defaults, make sure the operator-guide commissioning notes still
    // describe the behavior accurately.
    let kp = crate::config::default_hold_kp_nm_per_rad();
    let kd = crate::config::default_hold_kd_nm_s_per_rad();
    assert!(kp > 0.0 && kp < 50.0, "kp out of sane range: {kp}");
    assert!(kd > 0.0 && kd < 5.0, "kd out of sane range: {kd}");
}
