//! Frame-aware angle newtypes (radians).
//!
//! See `docs/decisions/0005-angle-units-and-frames.md`. Inventory and HTTP use plain
//! `f32` radians at the edge; motion paths use these types.

use serde::{Deserialize, Serialize};

use crate::can::math::{shortest_signed_delta as delta_f32, wrap_to_pi as wrap_f32};

/// Unsigned magnitude in radians (e.g. tolerance windows).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Radians(pub f32);

/// A signed angular delta in radians (shortest path between principal angles,
/// or linear delta along an unwrapped branch — context-dependent).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RadiansDelta(pub f32);

/// Principal-angle value in (−π, π], e.g. inventory home targets and band limits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PrincipalAngle(f32);

/// Absolute mechanical angle on the firmware/encoder continuity branch (may span
/// multiple turns within ±4π for type-2 feedback).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UnwrappedAngle(pub f32);

/// Angular rate in rad/s.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RadiansPerSecond(pub f32);

impl Radians {
    #[must_use]
    pub fn new(rad: f32) -> Self {
        Self(rad)
    }

    #[must_use]
    pub fn raw(self) -> f32 {
        self.0
    }

    #[must_use]
    pub fn to_principal(self) -> PrincipalAngle {
        PrincipalAngle::from_wrapped_rad(self.0)
    }
}

impl RadiansDelta {
    #[must_use]
    pub fn new(rad: f32) -> Self {
        Self(rad)
    }

    #[must_use]
    pub fn raw(self) -> f32 {
        self.0
    }

    #[must_use]
    pub fn abs(self) -> Radians {
        Radians(self.0.abs())
    }
}

impl PrincipalAngle {
    /// Wrap any radians into (−π, π].
    #[must_use]
    pub fn from_wrapped_rad(rad: f32) -> Self {
        Self(wrap_f32(rad))
    }

    #[must_use]
    pub fn raw(self) -> f32 {
        self.0
    }
}

impl UnwrappedAngle {
    #[must_use]
    pub fn new(rad: f32) -> Self {
        Self(rad)
    }

    #[must_use]
    pub fn raw(self) -> f32 {
        self.0
    }

    #[must_use]
    pub fn to_principal(self) -> PrincipalAngle {
        PrincipalAngle::from_wrapped_rad(self.0)
    }

    /// Shortest signed delta from this reading to `target` (principal), then advance
    /// the unwrapped scalar by that delta — canonical home-ramp target construction.
    #[must_use]
    pub fn toward_principal_home(self, target_principal: PrincipalAngle) -> Self {
        let d = delta_f32(self.0, target_principal.raw());
        Self(self.0 + d)
    }
}

impl RadiansPerSecond {
    #[must_use]
    pub fn new(rad_s: f32) -> Self {
        Self(rad_s)
    }

    #[must_use]
    pub fn raw(self) -> f32 {
        self.0
    }
}

impl From<f32> for UnwrappedAngle {
    fn from(rad: f32) -> Self {
        Self(rad)
    }
}

/// Shortest signed delta between two points given as raw readings (unwraps internally).
#[must_use]
pub fn shortest_signed_delta(current: UnwrappedAngle, target: UnwrappedAngle) -> RadiansDelta {
    RadiansDelta::new(delta_f32(current.raw(), target.raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::can::math::shortest_signed_delta as delta_f32;

    #[test]
    fn principal_wraps() {
        let p = PrincipalAngle::from_wrapped_rad(3.5 * std::f32::consts::PI);
        assert!((p.raw() - (-0.5 * std::f32::consts::PI)).abs() < 1e-5);
    }

    #[test]
    fn toward_home_shortest_path() {
        let from = UnwrappedAngle::new(6.0);
        let home = PrincipalAngle::from_wrapped_rad(0.0);
        let u = from.toward_principal_home(home);
        let d = delta_f32(from.raw(), home.raw());
        assert!((u.raw() - (from.raw() + d)).abs() < 1e-5);
    }
}
