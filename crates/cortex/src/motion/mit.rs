//! MIT streaming helpers: per-tick step clamp and gain resolution.

use crate::can::motion::shortest_signed_delta;
use crate::config::SafetyConfig;
use crate::inventory::Actuator;

/// Clamp `proposed_rad` so the shortest-path delta from `current_rad` is at
/// most `max_step_rad` (magnitude).
#[must_use]
pub fn clamp_mit_step(current_rad: f32, proposed_rad: f32, max_step_rad: f32) -> f32 {
    let d = shortest_signed_delta(current_rad, proposed_rad);
    let d = d.clamp(-max_step_rad, max_step_rad);
    current_rad + d
}

#[must_use]
pub fn mit_step_max_rad(motor: &Actuator, safety: &SafetyConfig) -> f32 {
    mit_step_max_rad_or(motor, safety.mit_max_angle_step_rad)
}

#[must_use]
pub fn mit_step_max_rad_or(motor: &Actuator, default_step_rad: f32) -> f32 {
    motor
        .common
        .mit_max_angle_step_rad
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(default_step_rad)
}

#[must_use]
pub fn mit_command_kp_kd(motor: &Actuator, safety: &SafetyConfig) -> (f32, f32) {
    mit_command_kp_kd_or(
        motor,
        safety.hold_kp_nm_per_rad,
        safety.hold_kd_nm_s_per_rad,
    )
}

#[must_use]
pub fn mit_command_kp_kd_or(motor: &Actuator, default_kp: f32, default_kd: f32) -> (f32, f32) {
    let kp = motor
        .common
        .mit_command_kp_nm_per_rad
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(default_kp);
    let kd = motor
        .common
        .mit_command_kd_nm_s_per_rad
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(default_kd);
    (kp, kd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_mit_step_respects_shortest_delta() {
        let cur = 0.0_f32;
        assert!((clamp_mit_step(cur, 0.5, 0.2) - 0.2).abs() < 1e-5);
        assert!((clamp_mit_step(cur, -0.5, 0.2) + 0.2).abs() < 1e-5);
        assert!((clamp_mit_step(3.0, 3.05, 0.2) - 3.05).abs() < 1e-4);
    }

    #[test]
    fn mit_step_max_prefers_inventory_override() {
        use crate::inventory::{Actuator, ActuatorCommon, ActuatorFamily, RobstrideModel};
        let common = ActuatorCommon {
            role: "m".into(),
            can_bus: "can0".into(),
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
            mit_max_angle_step_rad: Some(0.03),
            limb: None,
            joint_kind: None,
            notes_yaml: None,
            desired_params: std::collections::BTreeMap::new(),
        };
        let motor = Actuator {
            common,
            family: ActuatorFamily::Robstride {
                model: RobstrideModel::Rs03,
            },
        };
        assert!((mit_step_max_rad_or(&motor, 0.087) - 0.03).abs() < 1e-6);
    }
}
