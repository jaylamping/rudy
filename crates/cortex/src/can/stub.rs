//! Non-Linux stub for [`super::RealCanHandle`]: same API surface, errors on real CAN.

use anyhow::Result;

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
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn save_to_flash(&self, _motor: &Actuator) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
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
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn refresh_all_params(&self, _state: &SharedState) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn poll_once(&self, _state: &SharedState) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }
}
