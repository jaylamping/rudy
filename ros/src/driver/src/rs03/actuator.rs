//! RS03 actuator instance: type-safe handle implementing [`crate::robstride::RsActuator`].
//!
//! Low-level encode/decode stays in [`super::mit`], [`super::session`], etc.

use std::io;

use crate::robstride::{sealed, RsActuator, RsModel};
use crate::socketcan_bus::CanBus;

use super::params;
use super::session;

/// RS03 motor on the CAN bus (host and motor IDs per ADR-0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rs03 {
    host_id: u8,
    motor_id: u8,
}

impl Rs03 {
    #[must_use]
    pub const fn new(host_id: u8, motor_id: u8) -> Self {
        Self { host_id, motor_id }
    }

    #[must_use]
    pub const fn host_id(self) -> u8 {
        self.host_id
    }

    #[must_use]
    pub const fn motor_id(self) -> u8 {
        self.motor_id
    }

    /// Safe shutdown: stop, run_mode = 0, spd_ref = 0.
    pub fn defang(&self, bus: &CanBus) -> io::Result<()> {
        session::defang_motor(bus, self.host_id, self.motor_id)
    }
}

impl sealed::Sealed for Rs03 {}

impl RsActuator for Rs03 {
    fn model(&self) -> RsModel {
        RsModel::Rs03
    }

    fn host_id(&self) -> u8 {
        self.host_id
    }

    fn motor_id(&self) -> u8 {
        self.motor_id
    }

    fn run_mode_velocity(&self) -> u8 {
        2
    }

    fn param_index_run_mode(&self) -> u16 {
        params::RUN_MODE
    }

    fn param_index_spd_ref(&self) -> u16 {
        params::SPD_REF
    }
}
