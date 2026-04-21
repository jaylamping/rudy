//! Limb grouping: kinematic ordering and per-limb quarantine.
//!
//! Each actuator in inventory may carry an optional `limb` field (free-form
//! string like `left_arm`, `right_leg`, `torso`) and an optional
//! `joint_kind` (constrained enum). The home-all orchestrator groups
//! actuators by `limb` and within each limb sorts by `joint_kind.home_order()`
//! so the proximal joint always homes before the distal one.
//!
//! Ordering ranges leave room to insert new joint kinds without renumbering:
//!   - 1-9   torso / spine (homed before everything else, sequentially)
//!   - 10-19 arm joints
//!   - 20-29 leg joints
//!   - 30-39 head / neck

pub mod health;
mod ordering;

pub use health::{
    boot_state_kind_snake, effective_limb_id, limb_quarantine_http, limb_status,
    require_limb_healthy, require_limb_healthy_http, sibling_quarantine_failures, LimbStatus,
};
pub use ordering::{ordered_motors_per_limb, ordered_motors_per_limb_owned};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Canonical position of a joint in the kinematic chain. Used to derive the
/// proximal-to-distal home order without operator input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    // Torso / spine — homed first across all limbs.
    WaistRotation,
    SpinePitch,
    // Arm joints (proximal -> distal).
    ShoulderPitch,
    ShoulderRoll,
    UpperArmYaw,
    ElbowPitch,
    ForearmRoll,
    WristPitch,
    WristYaw,
    WristRoll,
    Gripper,
    // Leg joints (proximal -> distal).
    HipYaw,
    HipRoll,
    HipPitch,
    KneePitch,
    AnklePitch,
    AnkleRoll,
    // Head / neck — homed after limbs (limbs are kinematically heavier).
    NeckPitch,
    NeckYaw,
}

impl JointKind {
    /// Proximal-to-distal rank within a limb. Lower = home first.
    pub fn home_order(self) -> u8 {
        match self {
            JointKind::WaistRotation => 1,
            JointKind::SpinePitch => 2,
            JointKind::ShoulderPitch => 10,
            JointKind::ShoulderRoll => 11,
            JointKind::UpperArmYaw => 12,
            JointKind::ElbowPitch => 13,
            JointKind::ForearmRoll => 14,
            JointKind::WristPitch => 15,
            JointKind::WristYaw => 16,
            JointKind::WristRoll => 17,
            JointKind::Gripper => 18,
            JointKind::HipYaw => 20,
            JointKind::HipRoll => 21,
            JointKind::HipPitch => 22,
            JointKind::KneePitch => 23,
            JointKind::AnklePitch => 24,
            JointKind::AnkleRoll => 25,
            JointKind::NeckPitch => 30,
            JointKind::NeckYaw => 31,
        }
    }

    /// Snake-case canonical form. Used to derive the role identifier
    /// `{limb}.{joint_kind_snake_case}`.
    pub fn as_snake_case(self) -> &'static str {
        match self {
            JointKind::WaistRotation => "waist_rotation",
            JointKind::SpinePitch => "spine_pitch",
            JointKind::ShoulderPitch => "shoulder_pitch",
            JointKind::ShoulderRoll => "shoulder_roll",
            JointKind::UpperArmYaw => "upper_arm_yaw",
            JointKind::ElbowPitch => "elbow_pitch",
            JointKind::ForearmRoll => "forearm_roll",
            JointKind::WristPitch => "wrist_pitch",
            JointKind::WristYaw => "wrist_yaw",
            JointKind::WristRoll => "wrist_roll",
            JointKind::Gripper => "gripper",
            JointKind::HipYaw => "hip_yaw",
            JointKind::HipRoll => "hip_roll",
            JointKind::HipPitch => "hip_pitch",
            JointKind::KneePitch => "knee_pitch",
            JointKind::AnklePitch => "ankle_pitch",
            JointKind::AnkleRoll => "ankle_roll",
            JointKind::NeckPitch => "neck_pitch",
            JointKind::NeckYaw => "neck_yaw",
        }
    }

    /// Is this joint part of the torso/spine pre-phase? Those joints get
    /// homed sequentially before any limb tasks spawn, to avoid a wobbling
    /// torso while arms are mid-ramp.
    pub fn is_torso(self) -> bool {
        self.home_order() < 10
    }
}

#[cfg(test)]
#[path = "limb_tests.rs"]
mod ordering_tests;
