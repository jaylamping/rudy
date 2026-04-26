//! Non-Linux stub for [`super::RealCanHandle`]: same API surface, errors on real CAN
//! except `read_add_offset` (returns `Ok(0.0)`), `set_velocity_setpoint`, and `stop`
//! (no-op `Ok` so tests can run `home_ramp` with `real_can = Some`).

use anyhow::Result;

use crate::can::angle::PrincipalAngle;
use crate::inventory::Actuator;
use crate::spec::ParamDescriptor;
use crate::state::SharedState;

/// Placeholder type for `cfg(not(target_os = "linux"))` builds.
#[derive(Debug)]
pub struct RealCanHandle;

#[allow(dead_code)]
impl RealCanHandle {
    pub fn write_param(
        &self,
        _motor: &Actuator,
        _desc: &ParamDescriptor,
        _value: &serde_json::Value,
        _save_after: bool,
    ) -> Result<serde_json::Value> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn enable(&self, _motor: &Actuator) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn stop(&self, _motor: &Actuator) -> Result<()> {
        Ok(())
    }

    pub fn clear_fault(&self, _motor: &Actuator) -> Result<()> {
        Ok(())
    }

    pub fn calibrate_encoder(&self, _motor: &Actuator) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn save_to_flash(&self, _motor: &Actuator) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn ensure_active_report_100hz(&self, _motor: &Actuator) -> Result<()> {
        Ok(())
    }

    pub fn set_zero(&self, _motor: &Actuator) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    /// Mock-CAN equivalent of Linux `LinuxCanCore::read_add_offset`.
    pub fn read_add_offset(&self, _state: &SharedState, _motor: &Actuator) -> Result<f32> {
        Ok(0.0)
    }

    pub fn write_add_offset_persisted(
        &self,
        _state: &SharedState,
        _motor: &Actuator,
        _value_rad: f32,
    ) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn set_velocity_setpoint(&self, _motor: &Actuator, _vel_rad_s: f32) -> Result<()> {
        // No-op success so non-Linux integration/unit tests can exercise
        // `home_ramp` with `real_can = Some` without a SocketCAN stack.
        Ok(())
    }

    pub fn set_position_hold(&self, _motor: &Actuator, _target: PrincipalAngle) -> Result<()> {
        Ok(())
    }

    pub fn set_mit_hold(
        &self,
        _motor: &Actuator,
        _target: PrincipalAngle,
        _kp_nm_per_rad: f32,
        _kd_nm_s_per_rad: f32,
    ) -> Result<()> {
        Ok(())
    }

    pub fn set_mit_command_stream(
        &self,
        _motor: &Actuator,
        _position_rad: f32,
        _velocity_rad_s: f32,
        _torque_ff_nm: f32,
        _kp_nm_per_rad: f32,
        _kd_nm_s_per_rad: f32,
    ) -> Result<()> {
        Ok(())
    }

    pub fn refresh_all_params(&self, _state: &SharedState) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn poll_once(&self, _state: &SharedState) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }
}
