//! Pins [`crate::config::SafetyConfig::target_dwell_ticks`] default and overlap with
//! `default_target_dwell_ticks` (see `config::safety`).

use crate::config::{default_target_dwell_ticks, default_target_tolerance_rad};

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
