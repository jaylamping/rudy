use anyhow::Result;

use crate::inventory::Actuator;
use crate::state::SharedState;

use super::LinuxCanCore;

impl LinuxCanCore {
    /// Velocity-mode setpoint. The worker thread implements smart
    /// re-arm: on the first frame after `state.enabled` does NOT
    /// contain the role, the worker writes `RUN_MODE = 2` + sends
    /// `cmd_enable` + writes `SPD_REF`. On every subsequent frame
    /// (`state.enabled` already contains the role), it writes only
    /// `SPD_REF`. Cuts steady-state jog traffic from 60 to 20 frames/s.
    ///
    /// Velocity is *clamped* to the firmware-level `limit_spd`
    /// envelope before forwarding so a misbehaving caller can't bypass
    /// the firmware guard via the REST layer.
    pub fn set_velocity_setpoint(&self, motor: &Actuator, vel_rad_s: f32) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        handle.set_velocity(
            self.host_id,
            motor.common.can_id,
            &motor.common.role,
            vel_rad_s,
        )?;
        Ok(())
    }

    /// RAM-write low torque AND speed limits for every present motor.
    pub fn seed_boot_low_limits(&self, state: &SharedState) {
        let motors: Vec<Actuator> = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .actuators()
            .filter(|m| m.common.present)
            .cloned()
            .collect();

        for motor in motors {
            let spec = state.spec_for(motor.robstride_model());
            let limit_torque_nm = spec
                .commissioning_defaults
                .get("limit_torque_nm")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32);
            let limit_spd_rad_s = spec
                .commissioning_defaults
                .get("limit_spd_rad_s")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32);

            if let (Some(t), Some(_)) = (limit_torque_nm, &motor.common.travel_limits) {
                if let Some(desc) = spec.firmware_limits.get("limit_torque") {
                    if let Err(e) = self.write_param(&motor, desc, &serde_json::json!(t), false) {
                        tracing::warn!(role = %motor.common.role, error = ?e, "boot-time limit_torque RAM write failed");
                    }
                }
            }
            if let Some(s) = limit_spd_rad_s {
                if let Some(desc) = spec.firmware_limits.get("limit_spd") {
                    if let Err(e) = self.write_param(&motor, desc, &serde_json::json!(s), false) {
                        tracing::warn!(role = %motor.common.role, error = ?e, "boot-time limit_spd RAM write failed");
                    }
                }
            }
        }
    }
}
