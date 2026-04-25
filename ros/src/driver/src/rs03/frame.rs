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

/// Motor CAN node id (1..=127) for passive bus observation, when the arbitration field
/// layout is unambiguous (ADR-0002).
///
/// Recognized comm types (RS03 / RobStride):
///
/// | comm  | name                       | node bits | notes                              |
/// |-------|----------------------------|-----------|------------------------------------|
/// | 0x00  | `GetDeviceId` reply        | 16..23    | device-info / boot announce        |
/// | 0x02  | `MotorFeedback`            | 16..23    | streaming type-2                   |
/// | 0x11  | `ReadParam` reply          | 8..15     | type-17 read response              |
/// | 0x15  | `FaultFeedback`            | 8..15     | unsolicited fault frames           |
/// | 0x16  | `SaveParams` ack           | 8..15     | echoed by some firmware revs       |
/// | 0x18  | `ActiveReport` reply       | 8..15     | runtime-state push (type-24)       |
///
/// Comm types with operator-supplied bytes in the source-id slot
/// (`OperationCtrl`, `Enable`, `Stop`, `WriteParam`, …) are deliberately
/// ignored so MIT-control torque bytes in bits 16..23 are never mistaken
/// for a node address. Outbound frames the host TXs are also not the
/// observer's job; the worker filters its own TX via
/// `CAN_RAW_RECV_OWN_MSGS=0`.
#[inline]
pub fn passive_observer_node_id(can_id: u32) -> Option<u8> {
    let raw = strip_eff_flag(can_id);
    let comm = comm_type_from_id(can_id);
    let node = match comm {
        // Source id is in bits 16..23 (device → host frames whose layout
        // mirrors the type-2 motor feedback header).
        c if c == CommType::GetDeviceId as u8 || c == CommType::MotorFeedback as u8 =>
        {
            ((raw >> 16) & 0xFF) as u8
        }
        // Source id is in bits 8..15 (type-17 reply convention reused by
        // fault and save-params acks).
        c if c == CommType::ReadParam as u8
            || c == CommType::ActiveReport as u8
            || c == CommType::FaultFeedback as u8
            || c == CommType::SaveParams as u8 =>
        {
            ((raw >> 8) & 0xFF) as u8
        }
        _ => return None,
    };
    if node == 0 || node > 0x7F {
        None
    } else {
        Some(node)
    }
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

    #[test]
    fn passive_observer_type2_src() {
        let id = 0x0208_FD08u32;
        assert_eq!(passive_observer_node_id(with_eff_flag(id)), Some(0x08));
    }

    #[test]
    fn passive_observer_type17_reply_motor() {
        // ReadParam reply layout: status @ 16..23, motor @ 8..15, host @ 0..7
        let raw = (0x11u32 << 24) | (0x01u32 << 16) | (0x55u32 << 8) | 0xFDu32;
        assert_eq!(passive_observer_node_id(with_eff_flag(raw)), Some(0x55));
    }

    #[test]
    fn passive_observer_skips_mit_op_control() {
        let raw = (CommType::OperationCtrl as u32) << 24;
        assert_eq!(passive_observer_node_id(with_eff_flag(raw)), None);
    }

    #[test]
    fn passive_observer_get_device_id_reply() {
        // Type-0 device-info reply: source node id in bits 16..23.
        let raw = (CommType::GetDeviceId as u32) << 24 | (0x42u32 << 16) | 0xFD;
        assert_eq!(passive_observer_node_id(with_eff_flag(raw)), Some(0x42));
    }

    #[test]
    fn passive_observer_fault_feedback() {
        // Type-21 fault frame: source node id in bits 8..15 (reply layout).
        let raw = (CommType::FaultFeedback as u32) << 24 | (0x21u32 << 8) | 0xFD;
        assert_eq!(passive_observer_node_id(with_eff_flag(raw)), Some(0x21));
    }

    #[test]
    fn passive_observer_active_report() {
        // Type-24 active-report push: source node id in bits 8..15.
        let raw = (CommType::ActiveReport as u32) << 24 | (0x33u32 << 8) | 0xFD;
        assert_eq!(passive_observer_node_id(with_eff_flag(raw)), Some(0x33));
    }

    #[test]
    fn passive_observer_save_params_ack() {
        let raw = (CommType::SaveParams as u32) << 24 | (0x55u32 << 8) | 0xFD;
        assert_eq!(passive_observer_node_id(with_eff_flag(raw)), Some(0x55));
    }

    #[test]
    fn passive_observer_rejects_zero_and_overflow() {
        // node id == 0 (broadcast slot) is not a real device
        let raw = (CommType::MotorFeedback as u32) << 24;
        assert_eq!(passive_observer_node_id(with_eff_flag(raw)), None);
        // node id > 0x7F is reserved
        let raw = (CommType::MotorFeedback as u32) << 24 | (0x80u32 << 16);
        assert_eq!(passive_observer_node_id(with_eff_flag(raw)), None);
    }

    #[test]
    fn passive_observer_skips_host_originated_writes() {
        for c in [
            CommType::OperationCtrl,
            CommType::Enable,
            CommType::Stop,
            CommType::SetZero,
            CommType::SetCanId,
            CommType::WriteParam,
        ] {
            let raw = (c as u32) << 24 | (0x42u32 << 16) | 0xFD;
            assert!(
                passive_observer_node_id(with_eff_flag(raw)).is_none(),
                "comm 0x{:02x} must not fire passive observer",
                c as u8,
            );
        }
    }
}
