//! Shared RobStride **RSxx** abstraction.
//!
//! In OOP terms, [RsActuator] is the **base type**; each product line is a concrete type
//! (crate::rs03::Rs03, future crate::rs02::Rs02, …) that **implements** this trait. Rust does
//! not use class inheritance; a sealed trait + concrete structs gives the same extension point
//! without duplicating protocol code across families.
//!
//! Protocol codecs and session functions stay in crate::rs03, crate::rs02, etc. This module
//! only holds what is **polymorphic** across lines: model identity and shared addressing.

pub(crate) mod sealed {
    /// Crate-private seal — only this crate may implement [super::RsActuator].
    pub trait Sealed {}
}

/// Product line identifier for RobStride actuators handled by this repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RsModel {
    Rs01,
    Rs02,
    Rs03,
    Rs04,
}

/// Common surface for every RobStride RSxx actuator **type** in this crate.
///
/// Do not introduce a standalone RS03 API without going through an Rs03 value that
/// implements this trait (and the same for other lines). Orchestration code should take
/// &dyn RsActuator or impl RsActuator where the line is not fixed at compile time.
pub trait RsActuator: sealed::Sealed {
    fn model(&self) -> RsModel;
    fn host_id(&self) -> u8;
    fn motor_id(&self) -> u8;

    /// `run_mode` firmware value for velocity closed-loop (RS03: `2`).
    fn run_mode_velocity(&self) -> u8;

    fn param_index_run_mode(&self) -> u16;
    fn param_index_spd_ref(&self) -> u16;
}

#[cfg(test)]
mod tests {
    use super::{RsActuator, RsModel};
    use crate::rs03::Rs03;

    #[test]
    fn rs03_dyn_dispatch() {
        let a = Rs03::new(0xFD, 0x08);
        let r: &dyn RsActuator = &a;
        assert_eq!(r.model(), RsModel::Rs03);
        assert_eq!(r.host_id(), 0xFD);
        assert_eq!(r.motor_id(), 0x08);
        assert_eq!(r.run_mode_velocity(), 2);
        assert_eq!(r.param_index_run_mode(), crate::rs03::params::RUN_MODE);
        assert_eq!(r.param_index_spd_ref(), crate::rs03::params::SPD_REF);
    }
}
