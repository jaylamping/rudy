//! 29-bit extended arbitration ID helpers (ADR-0002).

use super::comm_types::CommType;

/// SocketCAN extended-frame flag OR'd into the 32-bit `can_id` field.
///
/// Single definition lives in [`crate::socketcan_bus`] so transport stays independent of RS03.
pub use crate::socketcan_bus::CAN_EFF_FLAG;

/// Build the 29-bit arbitration ID: `[comm_type(5)][host(8)][motor(8)]` with bits 23..16 zero.
///
/// For MIT operation control, [`super::mit::RobstrideCodec::encode_mit`] replaces bits 23..16 with
/// the torque feed-forward byte (see ADR-0002).
#[inline]
pub fn arb_id(comm: CommType, host_id: u8, motor_id: u8) -> u32 {
    let c = comm as u32 & 0x1F;
    (c << 24) | ((host_id as u32) << 8) | (motor_id as u32) & 0x1FFF_FFFF
}

#[inline]
pub fn with_eff_flag(raw_29: u32) -> u32 {
    (raw_29 & 0x1FFF_FFFF) | CAN_EFF_FLAG
}

#[inline]
pub fn strip_eff_flag(id: u32) -> u32 {
    id & 0x1FFF_FFFF
}

#[inline]
pub fn comm_type_from_id(id: u32) -> u8 {
    ((strip_eff_flag(id) >> 24) & 0x1F) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arb_id_matches_python_reference() {
        // tools/robstride/rs03_can.arb_id(0x11, 0xFD, 0x08) == 0x1100FD08
        let id = arb_id(CommType::ReadParam, 0xFD, 0x08);
        assert_eq!(id, 0x1100_FD08);
    }

    #[test]
    fn comm_type_roundtrip() {
        let id = arb_id(CommType::WriteParam, 0xAB, 0xCD);
        assert_eq!(comm_type_from_id(id), CommType::WriteParam as u8);
    }
}
