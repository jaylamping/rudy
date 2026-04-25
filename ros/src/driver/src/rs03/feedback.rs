//! Type-2 motor feedback decode (ADR-0002, vendor manual section 4.1.3).

use super::errors::ProtocolError;
use super::frame::strip_eff_flag;

// Match the vendor's 4*pi rad position range. We compute from PI rather
// than a literal so we don't trip the f32 `excessive_precision` lint
// (12.566_370_614 has more digits than f32 can faithfully represent).
const POS_LO: f32 = -4.0 * std::f32::consts::PI;
const POS_HI: f32 = 4.0 * std::f32::consts::PI;
const VEL_LO: f32 = -20.0;
const VEL_HI: f32 = 20.0;
const TORQUE_LO: f32 = -60.0;
const TORQUE_HI: f32 = 60.0;

#[inline]
fn u16_to_range(raw: u16, lo: f32, hi: f32) -> f32 {
    let x = (raw as f32) / 65535.0;
    lo + x * (hi - lo)
}

/// Decoded type-2 motor feedback (host must verify comm type == 2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotorFeedback {
    pub src_motor: u8,
    pub dest_host: u8,
    pub status_byte: u8,
    pub pos_rad: f32,
    pub vel_rad_s: f32,
    pub torque_nm: f32,
    pub temp_c: f32,
    pub raw_pos: u16,
    pub raw_vel: u16,
    pub raw_torque: u16,
    pub raw_temp: u16,
}

pub fn decode_motor_feedback(can_id: u32, data: &[u8]) -> Result<MotorFeedback, ProtocolError> {
    if data.len() < 8 {
        return Err(ProtocolError::ShortFrame {
            got: data.len(),
            need: 8,
        });
    }
    let raw_id = strip_eff_flag(can_id);
    let src_motor = ((raw_id >> 16) & 0xFF) as u8;
    let status_byte = ((raw_id >> 8) & 0xFF) as u8;
    let dest_host = (raw_id & 0xFF) as u8;

    let raw_pos = u16::from_be_bytes([data[0], data[1]]);
    let raw_vel = u16::from_be_bytes([data[2], data[3]]);
    let raw_torque = u16::from_be_bytes([data[4], data[5]]);
    let raw_temp = u16::from_be_bytes([data[6], data[7]]);

    Ok(MotorFeedback {
        src_motor,
        dest_host,
        status_byte,
        pos_rad: u16_to_range(raw_pos, POS_LO, POS_HI),
        vel_rad_s: u16_to_range(raw_vel, VEL_LO, VEL_HI),
        torque_nm: u16_to_range(raw_torque, TORQUE_LO, TORQUE_HI),
        temp_c: (raw_temp as f32) / 10.0,
        raw_pos,
        raw_vel,
        raw_torque,
        raw_temp,
    })
}

/// Decode a type-24 active-report telemetry frame.
///
/// Motor Tool emits active-report telemetry as `0x18SSMMFD`: status in
/// bits 16..23, source motor in bits 8..15, host in bits 0..7. The 8-byte
/// payload uses the same position / velocity / torque / temperature packing
/// as type-2 feedback.
pub fn decode_active_report_feedback(
    can_id: u32,
    data: &[u8],
) -> Result<MotorFeedback, ProtocolError> {
    if data.len() < 8 {
        return Err(ProtocolError::ShortFrame {
            got: data.len(),
            need: 8,
        });
    }
    let raw_id = strip_eff_flag(can_id);
    let status_byte = ((raw_id >> 16) & 0xFF) as u8;
    let src_motor = ((raw_id >> 8) & 0xFF) as u8;
    let dest_host = (raw_id & 0xFF) as u8;

    let raw_pos = u16::from_be_bytes([data[0], data[1]]);
    let raw_vel = u16::from_be_bytes([data[2], data[3]]);
    let raw_torque = u16::from_be_bytes([data[4], data[5]]);
    let raw_temp = u16::from_be_bytes([data[6], data[7]]);

    Ok(MotorFeedback {
        src_motor,
        dest_host,
        status_byte,
        pos_rad: u16_to_range(raw_pos, POS_LO, POS_HI),
        vel_rad_s: u16_to_range(raw_vel, VEL_LO, VEL_HI),
        torque_nm: u16_to_range(raw_torque, TORQUE_LO, TORQUE_HI),
        temp_c: (raw_temp as f32) / 10.0,
        raw_pos,
        raw_vel,
        raw_torque,
        raw_temp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_type2_midscale() {
        // All0x8000 -> mid of linear maps for pos/vel/torque; temp raw 250 -> 25C
        let can_id = 0x0208_FD08u32; // type 2, src0x08, status 0xFD, host 0x08 - illustrative
                                     // MOS temp: uint16 BE, degC = raw / 10 → 25 C => raw = 250 = 0x00FA
        let data = [0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x00, 0xFA];
        let fb = decode_motor_feedback(can_id, &data).unwrap();
        assert!((fb.pos_rad - 0.0).abs() < 1e-3);
        assert!((fb.vel_rad_s - 0.0).abs() < 1e-3);
        assert!((fb.torque_nm - 0.0).abs() < 1e-3);
        assert!((fb.temp_c - 25.0).abs() < 1e-3);
    }

    #[test]
    fn decode_type2_too_short_errors() {
        let err = decode_motor_feedback(0, &[0u8; 4]).unwrap_err();
        assert_eq!(err, ProtocolError::ShortFrame { got: 4, need: 8 });
    }

    #[test]
    fn decode_active_report_uses_reply_id_layout() {
        let can_id = 0x1880_08FDu32;
        let data = [0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x01, 0x18];
        let fb = decode_active_report_feedback(can_id, &data).unwrap();
        assert_eq!(fb.status_byte, 0x80);
        assert_eq!(fb.src_motor, 0x08);
        assert_eq!(fb.dest_host, 0xFD);
        assert_eq!(fb.raw_temp, 0x0118);
    }
}
