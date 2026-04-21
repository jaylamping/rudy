//! Wire types for motion intent (`MotionIntent`).
//!
//! See `crate::motion::status` for `MotionStatus`, `MotionState`, and
//! `MotionStopReason`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Closed-loop motion pattern the operator asked for. Carried in the
/// `MotionRegistry` for the lifetime of one run; the per-pattern step
/// function in [`crate::motion::patterns::sweep`] /
/// [`crate::motion::patterns::wave`] reads it on every tick.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MotionIntent {
    /// Sweep the full configured travel band, reversing just shy of each
    /// edge. The controller reads `travel_limits` from inventory each tick
    /// (so a mid-run band edit is honored) and clamps the velocity to the
    /// firmware envelope.
    Sweep {
        speed_rad_s: f32,
        turnaround_rad: f32,
    },
    /// Symmetric oscillation around `center_rad` with `amplitude_rad`
    /// half-swing. The center is captured at start and clipped against
    /// the band so a mid-run band shrink doesn't push the wave outside.
    Wave {
        center_rad: f32,
        amplitude_rad: f32,
        speed_rad_s: f32,
        turnaround_rad: f32,
    },
    /// Hold-to-jog: drive at `vel_rad_s` for as long as the operator's
    /// dead-man signal stays alive.
    Jog { vel_rad_s: f32 },
}

impl MotionIntent {
    /// snake_case discriminator used in the audit log and `MotionStatus`
    /// envelope. Stable wire identity even if the variants gain fields.
    pub fn kind_str(&self) -> &'static str {
        match self {
            MotionIntent::Sweep { .. } => "sweep",
            MotionIntent::Wave { .. } => "wave",
            MotionIntent::Jog { .. } => "jog",
        }
    }
}

/// Base turnaround inset, in rad. Covers the algorithmic margin (the
/// step function flips direction when `pos >= edge - inset`, so we need
/// at least one tick's worth of headroom even at v=0). Sized to match
/// the original fixed defaults so a low-speed sweep behaves exactly as
/// before this scaling was introduced.
pub const SWEEP_BASE_INSET_RAD: f32 = 0.05;
pub const WAVE_BASE_INSET_RAD: f32 = 0.02;

/// Per-rad/s overshoot allowance, in seconds. Multiplied by the
/// commanded speed to estimate how far the motor will coast past the
/// turnaround threshold before the velocity loop reverses it.
pub const OVERSHOOT_S: f32 = 0.15;

/// Resolve the default turnaround inset for a given pattern when the
/// REST handler / client frame omits it.
pub fn default_turnaround_rad(kind: &MotionIntent, speed_rad_s: f32) -> f32 {
    let speed = speed_rad_s.abs();
    let base = match kind {
        MotionIntent::Sweep { .. } => SWEEP_BASE_INSET_RAD,
        MotionIntent::Wave { .. } => WAVE_BASE_INSET_RAD,
        MotionIntent::Jog { .. } => return 0.0,
    };
    base + speed * OVERSHOOT_S
}
