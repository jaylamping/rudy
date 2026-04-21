use anyhow::Result;

use crate::inventory::Actuator;

use super::LinuxCanCore;

impl LinuxCanCore {
    pub fn enable(&self, motor: &Actuator) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        handle.enable(self.host_id, motor.common.can_id)?;
        Ok(())
    }

    pub fn stop(&self, motor: &Actuator) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        handle.stop(self.host_id, motor.common.can_id)?;
        Ok(())
    }

    pub fn save_to_flash(&self, motor: &Actuator) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        handle.save_params(self.host_id, motor.common.can_id)?;
        Ok(())
    }

    pub fn set_zero(&self, motor: &Actuator) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        handle.set_zero(self.host_id, motor.common.can_id)?;
        Ok(())
    }
}
