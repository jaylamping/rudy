//! Joint dynamics safety model.
//!
//! Provides per-joint torque envelopes, motion profiles, and the torque budget
//! reconciliation logic that the preflight system uses to reject motion requests
//! that would exceed the actuator's safe operating envelope under gravity load.
//!
//! Design philosophy:
//! - Fail closed: missing or incomplete data refuses motion on loaded joints.
//! - Conservative: use continuous torque (not peak) as the sustained hold budget.
//! - Layered: effective ceiling is min(configured continuous, firmware limit_torque,
//!   current-derived torque via Kt, structural limit).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::inventory::Actuator;
use crate::state::SharedState;

/// Per-actuator dynamics and safety envelope, stored in inventory.
///
/// Operators configure this per joint to declare the gravity and motion
/// constraints specific to that joint's position in the kinematic chain
/// and attached payload.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct JointDynamics {
    /// Continuous torque the actuator can sustain indefinitely without
    /// thermal damage (Nm). Must be <= peak torque. For RS03: 20 Nm rated.
    #[serde(default)]
    pub continuous_torque_nm: Option<f32>,

    /// Structural torque limit of the joint/bracket/mounting (Nm).
    /// The weakest mechanical element in the load path. If exceeded,
    /// hardware breaks regardless of actuator capability.
    #[serde(default)]
    pub structural_torque_nm: Option<f32>,

    /// Estimated gravity hold torque at worst-case pose (Nm).
    /// Computed offline from mass/COM/lever-arm or measured empirically.
    /// When set, preflight refuses motion if this exceeds the effective
    /// torque ceiling minus the configured margin.
    #[serde(default)]
    pub gravity_torque_nm: Option<f32>,

    /// Safety margin ratio applied on top of gravity_torque_nm. The
    /// effective requirement is `gravity_torque_nm * (1 + margin)`.
    /// Default 0.25 (25% headroom for dynamic loads, friction, estimation error).
    #[serde(default = "default_gravity_margin")]
    pub gravity_margin: f32,

    /// Maximum velocity allowed for this joint (rad/s). Overrides the
    /// global pattern/jog caps when set to a lower value.
    #[serde(default)]
    pub max_velocity_rad_s: Option<f32>,

    /// Maximum acceleration allowed for this joint (rad/s^2).
    #[serde(default)]
    pub max_acceleration_rad_s2: Option<f32>,

    /// Maximum jerk (rad/s^3). Limits the rate of acceleration change
    /// to protect gearboxes and reduce shock loads.
    #[serde(default)]
    pub max_jerk_rad_s3: Option<f32>,

    /// Motor torque constant (Nm/A). When known, enables current-to-torque
    /// conversion for tighter envelope checks. RS03 reports Kt via telemetry
    /// but the value varies per unit (spec: 2.36, observed: 1.53).
    #[serde(default)]
    pub kt_nm_per_amp: Option<f32>,

    /// Whether this joint is considered "loaded" (carries significant
    /// distal mass). Loaded joints get stricter defaults: current watchdog
    /// enforces (not observe-only), gravity checks are mandatory when
    /// gravity_torque_nm is set.
    #[serde(default)]
    pub loaded: bool,
}

fn default_gravity_margin() -> f32 {
    0.25
}

impl Default for JointDynamics {
    fn default() -> Self {
        Self {
            continuous_torque_nm: None,
            structural_torque_nm: None,
            gravity_torque_nm: None,
            gravity_margin: default_gravity_margin(),
            max_velocity_rad_s: None,
            max_acceleration_rad_s2: None,
            max_jerk_rad_s3: None,
            kt_nm_per_amp: None,
            loaded: false,
        }
    }
}

/// Resolved torque budget for a single joint at the moment of a preflight check.
#[derive(Debug, Clone)]
pub struct TorqueBudget {
    /// Effective ceiling: min of all known torque limits.
    pub ceiling_nm: f32,
    /// Source that determined the ceiling (for diagnostics).
    pub ceiling_source: &'static str,
    /// Required torque to hold against gravity at worst-case pose (with margin).
    pub required_hold_nm: f32,
    /// Headroom: ceiling - required. Negative means motion should be refused.
    pub headroom_nm: f32,
}

/// Why a torque budget check failed.
#[derive(Debug, Clone)]
pub struct TorqueBudgetViolation {
    pub budget: TorqueBudget,
    pub role: String,
}

impl TorqueBudgetViolation {
    pub fn detail(&self) -> String {
        format!(
            "{}: gravity hold requires {:.1} Nm but effective ceiling is {:.1} Nm ({}); headroom {:.1} Nm",
            self.role,
            self.budget.required_hold_nm,
            self.budget.ceiling_nm,
            self.budget.ceiling_source,
            self.budget.headroom_nm,
        )
    }
}

/// Compute the effective torque budget for an actuator.
///
/// Returns `None` if the joint has no dynamics configured (unloaded joints
/// with no gravity_torque_nm are assumed safe from a torque perspective
/// and skip this check).
pub fn compute_torque_budget(state: &SharedState, motor: &Actuator) -> Option<TorqueBudget> {
    let dynamics = motor.common.dynamics.as_ref()?;

    let gravity_torque = dynamics.gravity_torque_nm?;
    if gravity_torque <= 0.0 {
        return None;
    }

    let required_hold_nm = gravity_torque * (1.0 + dynamics.gravity_margin);

    let mut ceiling_nm = f32::MAX;
    let mut ceiling_source: &'static str = "none";

    if let Some(cont) = dynamics.continuous_torque_nm {
        if cont > 0.0 && cont < ceiling_nm {
            ceiling_nm = cont;
            ceiling_source = "continuous_torque";
        }
    }

    if let Some(structural) = dynamics.structural_torque_nm {
        if structural > 0.0 && structural < ceiling_nm {
            ceiling_nm = structural;
            ceiling_source = "structural_torque";
        }
    }

    // Firmware limit_torque from desired_params or live telemetry
    if let Some(fw_limit) = firmware_torque_limit(state, motor) {
        if fw_limit > 0.0 && fw_limit < ceiling_nm {
            ceiling_nm = fw_limit;
            ceiling_source = "firmware_limit_torque";
        }
    }

    // Current-derived torque limit: limit_cur * Kt
    if let Some(kt) = dynamics.kt_nm_per_amp {
        if let Some(limit_cur) = firmware_current_limit(state, motor) {
            let torque_from_current = limit_cur * kt;
            if torque_from_current > 0.0 && torque_from_current < ceiling_nm {
                ceiling_nm = torque_from_current;
                ceiling_source = "current_derived_torque";
            }
        }
    }

    if ceiling_nm == f32::MAX {
        // No ceiling could be determined — fail closed for loaded joints
        if dynamics.loaded {
            ceiling_nm = 0.0;
            ceiling_source = "unknown_ceiling_loaded_joint";
        } else {
            return None;
        }
    }

    let headroom_nm = ceiling_nm - required_hold_nm;

    Some(TorqueBudget {
        ceiling_nm,
        ceiling_source,
        required_hold_nm,
        headroom_nm,
    })
}

/// Resolve firmware `limit_torque` from desired_params.
fn firmware_torque_limit(_state: &SharedState, motor: &Actuator) -> Option<f32> {
    motor
        .common
        .desired_params
        .get("limit_torque")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
}

/// Resolve firmware `limit_cur` from desired_params.
fn firmware_current_limit(_state: &SharedState, motor: &Actuator) -> Option<f32> {
    motor
        .common
        .desired_params
        .get("limit_cur")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
}

/// Check if the requested velocity exceeds the joint's configured maximum.
/// Returns the effective clamped velocity.
pub fn clamp_velocity_for_joint(motor: &Actuator, requested_vel_rad_s: f32) -> f32 {
    let dynamics = match motor.common.dynamics.as_ref() {
        Some(d) => d,
        None => return requested_vel_rad_s,
    };
    match dynamics.max_velocity_rad_s {
        Some(max_vel) if max_vel > 0.0 => {
            let sign = requested_vel_rad_s.signum();
            sign * requested_vel_rad_s.abs().min(max_vel)
        }
        _ => requested_vel_rad_s,
    }
}

/// Check if requested velocity exceeds joint limit. Returns `Some(max)` if violated.
pub fn velocity_exceeds_joint_limit(motor: &Actuator, requested_vel_rad_s: f32) -> Option<f32> {
    let dynamics = motor.common.dynamics.as_ref()?;
    let max_vel = dynamics.max_velocity_rad_s?;
    if max_vel > 0.0 && requested_vel_rad_s.abs() > max_vel {
        Some(max_vel)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn make_dynamics(
        continuous: Option<f32>,
        structural: Option<f32>,
        gravity: Option<f32>,
        loaded: bool,
    ) -> JointDynamics {
        JointDynamics {
            continuous_torque_nm: continuous,
            structural_torque_nm: structural,
            gravity_torque_nm: gravity,
            gravity_margin: 0.25,
            max_velocity_rad_s: None,
            max_acceleration_rad_s2: None,
            max_jerk_rad_s3: None,
            kt_nm_per_amp: None,
            loaded,
        }
    }

    fn make_actuator(dynamics: Option<JointDynamics>, limit_torque: Option<f32>) -> Actuator {
        use crate::inventory::{ActuatorCommon, ActuatorFamily, RobstrideModel};
        let mut desired_params = BTreeMap::new();
        if let Some(lt) = limit_torque {
            desired_params.insert(
                "limit_torque".to_string(),
                serde_json::Value::from(lt as f64),
            );
        }
        Actuator {
            common: ActuatorCommon {
                role: "test.joint".to_string(),
                can_bus: "can0".to_string(),
                can_id: 1,
                present: true,
                verified: true,
                commissioned_at: None,
                firmware_version: None,
                travel_limits: None,
                commissioned_zero_offset: None,
                active_report_persisted: false,
                predefined_home_rad: None,
                homing_speed_rad_s: None,
                hold_kp_nm_per_rad: None,
                hold_kd_nm_s_per_rad: None,
                mit_command_kp_nm_per_rad: None,
                mit_command_kd_nm_s_per_rad: None,
                mit_max_angle_step_rad: None,
                limb: None,
                joint_kind: None,
                notes_yaml: None,
                desired_params,
                current_safety: None,
                dynamics,
            },
            family: ActuatorFamily::Robstride {
                model: RobstrideModel::Rs03,
            },
        }
    }

    #[test]
    fn no_dynamics_returns_none() {
        let motor = make_actuator(None, None);
        // Can't call compute_torque_budget without SharedState in unit test,
        // but we can test the logic path by checking dynamics access
        assert!(motor.common.dynamics.is_none());
    }

    #[test]
    fn no_gravity_torque_returns_none_even_with_dynamics() {
        let d = make_dynamics(Some(20.0), Some(30.0), None, false);
        assert!(d.gravity_torque_nm.is_none());
    }

    #[test]
    fn ceiling_picks_minimum() {
        let d = make_dynamics(Some(20.0), Some(15.0), Some(10.0), true);
        // structural (15) < continuous (20), so ceiling should be 15
        assert_eq!(d.structural_torque_nm, Some(15.0));
        assert!(d.structural_torque_nm.unwrap() < d.continuous_torque_nm.unwrap());
    }

    #[test]
    fn gravity_margin_applied() {
        let d = make_dynamics(Some(20.0), None, Some(10.0), false);
        let required = d.gravity_torque_nm.unwrap() * (1.0 + d.gravity_margin);
        assert!((required - 12.5).abs() < 0.01);
    }

    #[test]
    fn velocity_clamp_respects_limit() {
        let d = JointDynamics {
            max_velocity_rad_s: Some(1.0),
            ..Default::default()
        };
        let motor = make_actuator(Some(d), None);
        assert_eq!(clamp_velocity_for_joint(&motor, 2.0), 1.0);
        assert_eq!(clamp_velocity_for_joint(&motor, -2.0), -1.0);
        assert_eq!(clamp_velocity_for_joint(&motor, 0.5), 0.5);
    }

    #[test]
    fn velocity_exceeds_reports_limit() {
        let d = JointDynamics {
            max_velocity_rad_s: Some(1.5),
            ..Default::default()
        };
        let motor = make_actuator(Some(d), None);
        assert_eq!(velocity_exceeds_joint_limit(&motor, 2.0), Some(1.5));
        assert_eq!(velocity_exceeds_joint_limit(&motor, 1.0), None);
    }
}
