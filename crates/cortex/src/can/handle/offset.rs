use anyhow::{anyhow, Result};

use crate::inventory::Actuator;
use crate::state::SharedState;

use super::{LinuxCanCore, PARAM_TIMEOUT};

impl LinuxCanCore {
    pub fn read_add_offset(&self, state: &SharedState, motor: &Actuator) -> Result<f32> {
        let spec = state.spec_for(motor.robstride_model());
        let desc = spec
            .firmware_limits
            .get("add_offset")
            .or_else(|| spec.observables.get("add_offset"))
            .ok_or_else(|| {
                anyhow!(
                    "add_offset not found in actuator spec (looked in firmware_limits and observables)"
                )
            })?;
        let handle = self.handle_for(&motor.common.can_bus)?;
        let bytes =
            handle.read_param(self.host_id, motor.common.can_id, desc.index, PARAM_TIMEOUT)?;
        bytes
            .map(f32::from_le_bytes)
            .ok_or_else(|| anyhow!("read_add_offset returned no value (firmware read-fail)"))
    }

    /// Write `add_offset` (0x702B) in RAM and issue SaveParams so it persists
    /// across power-off. Used by `POST /restore_offset` to push the inventory
    /// commissioning record back to the firmware.
    pub fn write_add_offset_persisted(
        &self,
        state: &SharedState,
        motor: &Actuator,
        value_rad: f32,
    ) -> Result<()> {
        let spec = state.spec_for(motor.robstride_model());
        let desc = spec
            .firmware_limits
            .get("add_offset")
            .or_else(|| spec.observables.get("add_offset"))
            .ok_or_else(|| {
                anyhow!(
                    "add_offset not found in actuator spec (looked in firmware_limits and observables)"
                )
            })?;
        self.write_param(motor, desc, &serde_json::json!(value_rad), true)?;
        Ok(())
    }
}
