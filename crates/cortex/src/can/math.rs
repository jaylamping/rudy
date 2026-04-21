//! Atomic angular-motion safety primitives.
//!
//! Every command-producing endpoint in cortex routes its target/current
//! angles through these two functions before deciding whether to dispatch a
//! CAN frame. Correctness here is load-bearing for the boot-time safety
//! gate (see `crate::boot_state` and `crate::can::travel`): a bug in
//! `wrap_to_pi` or `shortest_signed_delta` would let the multi-turn-encoder
//! disaster (firmware reports +20 deg actually meaning +20 deg + 360 deg,
//! operator commands "go to 0 deg", motor takes the -340 deg path and rips
//! out wiring) bypass the band check.
//!
//! All RS03 joints in this robot are cable-bound to strictly less than one
//! full revolution of output range, so the principal-angle representation
//! in [-pi, +pi] is unambiguous within each joint's physical envelope. Any
//! future joint that genuinely needs continuous rotation (a wheel, a turret)
//! must NOT use these primitives without first checking the joint kinematics
//! flag — see "Open follow-ups" in the boot-time-gate plan.

use std::f32::consts::{PI, TAU};

/// Reduce an angle to its principal value in [-pi, +pi].
///
/// Non-finite inputs (NaN, +/- infinity) saturate to 0.0 — callers should
/// reject the request upstream rather than relying on this; the saturation
/// is purely a "fail safe, not silent" guarantee for the inner loop.
pub fn wrap_to_pi(rad: f32) -> f32 {
    if !rad.is_finite() {
        return 0.0;
    }
    let mut x = rad % TAU;
    if x > PI {
        x -= TAU;
    }
    if x < -PI {
        x += TAU;
    }
    x
}

/// Shortest signed angular distance from `current` to `target`, in [-pi, +pi].
///
/// Both inputs are reduced to principal angles first, so the result is the
/// smallest rotation (in either direction) that lands the motor at the
/// target. This is the canonical input to a per-step ceiling check — any
/// motion command should compare `shortest_signed_delta(current, target).abs()`
/// against the configured per-step cap rather than `(target - current).abs()`.
pub fn shortest_signed_delta(current_rad: f32, target_rad: f32) -> f32 {
    wrap_to_pi(wrap_to_pi(target_rad) - wrap_to_pi(current_rad))
}

#[cfg(test)]
#[path = "math_tests.rs"]
mod tests;
