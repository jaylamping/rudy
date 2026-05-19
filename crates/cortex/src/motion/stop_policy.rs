//! Stop-behavior policy for gravity-loaded joints.
//!
//! When a motion controller exits, the default behavior is hard-stop
//! (`cmd_stop` = torque-off). For joints that carry distal mass (shoulder,
//! elbow), torque-off causes free-fall and collision. This module provides
//! a per-actuator override: instead of torque-off, issue an MIT hold command
//! at the current position so the joint stays put under gravity.
//!
//! The policy is conservative:
//! - Only graceful stop reasons (operator, superseded, client_gone, shutdown)
//!   are eligible for hold. Faults always hard-stop.
//! - The actuator must be homed (boot state) — hold at an unknown position
//!   could mask a mechanical issue.
//! - The actuator must explicitly opt in via `stop_behavior: hold` in inventory.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::boot_state::BootState;
use crate::motion::status::MotionStopReason;

/// Per-actuator stop behavior selection. Stored in inventory YAML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum StopBehavior {
    /// Torque-off (`cmd_stop`). Safe for unloaded joints. Default.
    #[default]
    HardStop,
    /// MIT hold at current position using configured kp/kd gains.
    /// Prevents gravity backdrive on loaded joints.
    Hold,
}

/// Resolved stop action after evaluating the policy matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopAction {
    /// Issue `cmd_stop` (torque-off).
    HardStop,
    /// Issue MIT hold at current position with configured gains.
    HoldPosition,
}

/// Evaluate stop policy given the configured behavior, stop reason, and boot state.
///
/// Returns `HoldPosition` only when ALL conditions are met:
/// 1. Actuator's configured `stop_behavior` is `Hold`
/// 2. Stop reason is graceful (operator, superseded, client_gone, shutdown)
/// 3. Boot state is `Homed`
///
/// All other combinations fall through to `HardStop`.
pub fn resolve(
    configured: StopBehavior,
    reason: &MotionStopReason,
    boot_state: Option<&BootState>,
) -> StopAction {
    if configured != StopBehavior::Hold {
        return StopAction::HardStop;
    }

    if !is_graceful_reason(reason) {
        return StopAction::HardStop;
    }

    match boot_state {
        Some(BootState::Homed) => StopAction::HoldPosition,
        _ => StopAction::HardStop,
    }
}

/// Graceful reasons where holding position is safe and desirable.
fn is_graceful_reason(reason: &MotionStopReason) -> bool {
    matches!(
        reason,
        MotionStopReason::Operator
            | MotionStopReason::Superseded
            | MotionStopReason::ClientGone
            | MotionStopReason::Shutdown
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_stop_config_always_hard_stops() {
        let action = resolve(
            StopBehavior::HardStop,
            &MotionStopReason::Operator,
            Some(&BootState::Homed),
        );
        assert_eq!(action, StopAction::HardStop);
    }

    #[test]
    fn hold_config_operator_homed_holds() {
        let action = resolve(
            StopBehavior::Hold,
            &MotionStopReason::Operator,
            Some(&BootState::Homed),
        );
        assert_eq!(action, StopAction::HoldPosition);
    }

    #[test]
    fn hold_config_superseded_homed_holds() {
        let action = resolve(
            StopBehavior::Hold,
            &MotionStopReason::Superseded,
            Some(&BootState::Homed),
        );
        assert_eq!(action, StopAction::HoldPosition);
    }

    #[test]
    fn hold_config_fault_always_hard_stops() {
        let action = resolve(
            StopBehavior::Hold,
            &MotionStopReason::StaleTelemetry,
            Some(&BootState::Homed),
        );
        assert_eq!(action, StopAction::HardStop);
    }

    #[test]
    fn hold_config_travel_violation_hard_stops() {
        let action = resolve(
            StopBehavior::Hold,
            &MotionStopReason::TravelLimitViolation,
            Some(&BootState::Homed),
        );
        assert_eq!(action, StopAction::HardStop);
    }

    #[test]
    fn hold_config_not_homed_hard_stops() {
        let action = resolve(
            StopBehavior::Hold,
            &MotionStopReason::Operator,
            Some(&BootState::InBand),
        );
        assert_eq!(action, StopAction::HardStop);
    }

    #[test]
    fn hold_config_no_boot_state_hard_stops() {
        let action = resolve(StopBehavior::Hold, &MotionStopReason::Operator, None);
        assert_eq!(action, StopAction::HardStop);
    }

    #[test]
    fn hold_config_current_trip_hard_stops() {
        let reason = MotionStopReason::CurrentTrip {
            tier: "severe".into(),
            current_arms: 8.0,
            threshold_arms: 7.0,
            duration_ms: 100,
        };
        let action = resolve(StopBehavior::Hold, &reason, Some(&BootState::Homed));
        assert_eq!(action, StopAction::HardStop);
    }

    #[test]
    fn hold_config_motor_fault_hard_stops() {
        let reason = MotionStopReason::MotorFault {
            fault_sta: 0x01,
            warn_sta: 0x00,
        };
        let action = resolve(StopBehavior::Hold, &reason, Some(&BootState::Homed));
        assert_eq!(action, StopAction::HardStop);
    }

    #[test]
    fn hold_config_client_gone_homed_holds() {
        let action = resolve(
            StopBehavior::Hold,
            &MotionStopReason::ClientGone,
            Some(&BootState::Homed),
        );
        assert_eq!(action, StopAction::HoldPosition);
    }

    #[test]
    fn hold_config_shutdown_homed_holds() {
        let action = resolve(
            StopBehavior::Hold,
            &MotionStopReason::Shutdown,
            Some(&BootState::Homed),
        );
        assert_eq!(action, StopAction::HoldPosition);
    }
}
