//! Pins [`crate::config::SafetyConfig::target_dwell_ticks`] default and overlap with
//! `default_target_dwell_ticks` (see `config::safety`).
//!
//! Also pins the companion velocity-gate default
//! [`crate::config::SafetyConfig::target_dwell_max_vel_rad_s`] added to fix the
//! handoff-coast-then-stiction-trap failure mode on auto-home (motor satisfied
//! the position deadband but still had residual velocity when the homer declared
//! success; during the ~20-50 ms `cmd_stop → write RUN_MODE → cmd_enable → MIT`
//! disabled window it coasted 2-15 mrad and stiction then held it wherever it
//! landed, producing a 0.8-1.2° run-to-run "home" offset on a gravity-neutral
//! single-actuator setup). Runtime behavior of the gate is pinned end-to-end in
//! `home_ramp_real_can_stub_tests::velocity_gate_*`.

use crate::config::{
    default_target_dwell_max_vel_rad_s, default_target_dwell_ticks, default_target_tolerance_rad,
};

#[test]
fn default_target_dwell_ticks_is_positive() {
    assert!(default_target_dwell_ticks() >= 1);
}

#[test]
fn tolerance_window_accommodates_single_tick_overshoot() {
    // Documented invariant: deadband should exceed per-tick travel; home_ramp enforces dwell separately.
    let step = crate::config::default_step_size_rad();
    assert!(
        default_target_tolerance_rad() > step,
        "target_tolerance should exceed step_size_rad to avoid limit-cycle bounce (see cortex.toml)"
    );
}

#[test]
fn default_dwell_velocity_gate_is_finite_and_conservative() {
    // The gate only matters if it's finite-positive: `home_ramp.rs` falls back
    // to INFINITY (i.e. position-only dwell, pre-fix behavior) for NaN/inf/≤0,
    // so the default has to be a real number.
    let v = default_target_dwell_max_vel_rad_s();
    assert!(
        v.is_finite() && v > 0.0,
        "velocity gate default must be finite-positive; got {v}"
    );

    // Upper sanity bound: the whole point of the gate is to catch residual
    // velocity (the firmware is already commanding vel=0 when the motor is
    // inside the position deadband, so any surviving |vel| reflects loop
    // lag / filter lag, not real motion). A default much above ~0.1 rad/s
    // defeats the gate's purpose — RS03's `spd_filt_gain=0.1` low-passes
    // reported velocity with ~100 ms time constant, so anything above 0.1
    // would let a genuinely coasting motor still look "settled".
    assert!(
        v <= 0.1,
        "velocity gate default too permissive to catch handoff coast: {v}"
    );

    // Lower sanity bound: tighter than ~10 mrad/s will flake on filter
    // noise (RS03 firmware quantization + the 0.1 low-pass noise floor
    // rides at a few mrad/s even at true zero).
    assert!(
        v >= 0.01,
        "velocity gate default tight enough to flake on filter noise: {v}"
    );
}
