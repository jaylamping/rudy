use anyhow::Result;
use driver::rs03::params::EPSCAN_TIME;

use crate::can::worker::WriteValue;
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

    pub fn clear_fault(&self, motor: &Actuator) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        handle.clear_fault(self.host_id, motor.common.can_id)?;
        Ok(())
    }

    pub fn calibrate_encoder(&self, motor: &Actuator) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        handle.calibrate_encoder(self.host_id, motor.common.can_id)?;
        Ok(())
    }

    pub fn save_to_flash(&self, motor: &Actuator) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        handle.save_params(self.host_id, motor.common.can_id)?;
        Ok(())
    }

    /// Ensure RS03 active feedback reporting runs at 100 Hz.
    ///
    /// Sequence:
    /// 1) write `EPScan_time` (0x7026) = 1 via type-18 (10 ms period)
    /// 2) send type-24 active-report enable
    pub fn ensure_active_report_100hz(&self, motor: &Actuator) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        handle.write_param(
            self.host_id,
            motor.common.can_id,
            EPSCAN_TIME,
            WriteValue::U8(1),
        )?;
        handle.active_report(self.host_id, motor.common.can_id, true)?;
        Ok(())
    }

    pub fn set_zero(&self, motor: &Actuator) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        handle.set_zero(self.host_id, motor.common.can_id)?;
        Ok(())
    }
}
