//! MIT operation-control (type 1) packing per ADR-0002.

use std::f32::consts::PI;

use super::comm_types::CommType;
use super::errors::ProtocolError;

const P_MIN: f32 = -4.0 * PI;
const P_MAX: f32 = 4.0 * PI;
const V_MIN: f32 = -20.0;
const V_MAX: f32 = 20.0;
const KP_MIN: f32 = 0.0;
const KP_MAX: f32 = 5000.0;
const KD_MIN: f32 = 0.0;
const KD_MAX: f32 = 100.0;
const T_MIN: f32 = -60.0;
const T_MAX: f32 = 60.0;

#[inline]
fn float_to_u16(x: f32, x_min: f32, x_max: f32) -> Result<u16, ProtocolError> {
    if x < x_min || x > x_max {
        return Err(ProtocolError::OutOfRange);
    }
    let span = x_max - x_min;
    let norm = (x - x_min) / span;
    let v = (norm * 65535.0).round();
    Ok(v.clamp(0.0, 65535.0) as u16)
}

#[inline]
fn u16_to_float(raw: u16, x_min: f32, x_max: f32) -> f32 {
    let norm = (raw as f32) / 65535.0;
    x_min + norm * (x_max - x_min)
}

/// Map torque feed-forward (Nm) into the ID byte at bits 23..16 (ADR-0002).
#[inline]
fn torque_ff_to_id_byte(t: f32) -> Result<u8, ProtocolError> {
    if !(T_MIN..=T_MAX).contains(&t) {
        return Err(ProtocolError::OutOfRange);
    }
    let norm = (t - T_MIN) / (T_MAX - T_MIN);
    Ok((norm * 255.0).round().clamp(0.0, 255.0) as u8)
}

#[inline]
fn id_byte_to_torque_ff(b: u8) -> f32 {
    let norm = (b as f32) / 255.0;
    T_MIN + norm * (T_MAX - T_MIN)
}

/// MIT-style command parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MitCommand {
    pub position_rad: f32,
    pub velocity_rad_s: f32,
    pub kp: f32,
    pub kd: f32,
    pub torque_ff_nm: f32,
}

/// Decoded MIT fields (payload + torque byte from arbitration ID).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecodedCommand {
    pub position_rad: f32,
    pub velocity_rad_s: f32,
    pub kp: f32,
    pub kd: f32,
    pub torque_ff_nm: f32,
}

/// Encode/decode helper (name kept for compatibility with earlier driver code).
#[derive(Debug, Default, Clone, Copy)]
pub struct RobstrideCodec;

impl RobstrideCodec {
    /// Encode MIT frame: 29-bit ID with torque in bits 23..16, 8-byte payload (4x uint16 BE).
    pub fn encode_mit(
        self,
        host_id: u8,
        motor_id: u8,
        cmd: MitCommand,
    ) -> Result<(u32, [u8; 8]), ProtocolError> {
        let t_byte = torque_ff_to_id_byte(cmd.torque_ff_nm)?;
        let p = float_to_u16(cmd.position_rad, P_MIN, P_MAX)?;
        let v = float_to_u16(cmd.velocity_rad_s, V_MIN, V_MAX)?;
        let kp = float_to_u16(cmd.kp, KP_MIN, KP_MAX)?;
        let kd = float_to_u16(cmd.kd, KD_MIN, KD_MAX)?;

        let id = ((CommType::OperationCtrl as u32) << 24)
            | ((t_byte as u32) << 16)
            | ((host_id as u32) << 8)
            | (motor_id as u32);

        let mut data = [0u8; 8];
        data[0..2].copy_from_slice(&p.to_be_bytes());
        data[2..4].copy_from_slice(&v.to_be_bytes());
        data[4..6].copy_from_slice(&kp.to_be_bytes());
        data[6..8].copy_from_slice(&kd.to_be_bytes());

        Ok((id & 0x1FFF_FFFF, data))
    }

    /// Decode 8-byte MIT payload (big-endian 16-bit fields).
    pub fn decode_mit_payload(self, data: &[u8; 8]) -> DecodedCommand {
        let p = u16::from_be_bytes([data[0], data[1]]);
        let v = u16::from_be_bytes([data[2], data[3]]);
        let kp = u16::from_be_bytes([data[4], data[5]]);
        let kd = u16::from_be_bytes([data[6], data[7]]);
        DecodedCommand {
            position_rad: u16_to_float(p, P_MIN, P_MAX),
            velocity_rad_s: u16_to_float(v, V_MIN, V_MAX),
            kp: u16_to_float(kp, KP_MIN, KP_MAX),
            kd: u16_to_float(kd, KD_MIN, KD_MAX),
            torque_ff_nm: 0.0,
        }
    }

    /// Decode full MIT command including torque feed-forward from arbitration ID bits 23..16.
    pub fn decode_mit(self, can_id: u32, data: &[u8; 8]) -> DecodedCommand {
        let mut d = self.decode_mit_payload(data);
        let raw = can_id & 0x1FFF_FFFF;
        let t_byte = ((raw >> 16) & 0xFF) as u8;
        d.torque_ff_nm = id_byte_to_torque_ff(t_byte);
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mit_encode_decode_roundtrip() {
        let codec = RobstrideCodec;
        let cmd = MitCommand {
            position_rad: 0.1,
            velocity_rad_s: -1.0,
            kp: 10.0,
            kd: 0.5,
            torque_ff_nm: 0.0,
        };
        let (id, payload) = codec.encode_mit(0xFD, 0x08, cmd).unwrap();
        let dec = codec.decode_mit(id, &payload);
        assert!((dec.position_rad - cmd.position_rad).abs() < 0.02);
        assert!((dec.velocity_rad_s - cmd.velocity_rad_s).abs() < 0.05);
        assert!((dec.kp - cmd.kp).abs() < 0.5);
        assert!((dec.kd - cmd.kd).abs() < 0.05);
        assert!((dec.torque_ff_nm - cmd.torque_ff_nm).abs() < 0.5);
    }

    #[test]
    fn out_of_range_position_errors() {
        let codec = RobstrideCodec;
        let cmd = MitCommand {
            position_rad: 100.0,
            velocity_rad_s: 0.0,
            kp: 1.0,
            kd: 0.1,
            torque_ff_nm: 0.0,
        };
        assert_eq!(codec.encode_mit(1, 1, cmd), Err(ProtocolError::OutOfRange));
    }

    /// Vendor manual section 4.4 style neutral command: zero vel, zero gains, mid position.
    #[test]
    fn vendor_style_neutral_mit_payload() {
        let codec = RobstrideCodec;
        let cmd = MitCommand {
            position_rad: 0.0,
            velocity_rad_s: 0.0,
            kp: 0.0,
            kd: 0.0,
            torque_ff_nm: 0.0,
        };
        let (_id, payload) = codec.encode_mit(0xFD, 0x08, cmd).unwrap();
        // Mid-scale position/velocity maps to 0x8000 (0 rad, 0 rad/s); kp/kd min -> 0x0000
        assert_eq!(
            &payload[..],
            &[0x80, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn kp_kd_ranges_use_adr_0002_scales() {
        let codec = RobstrideCodec;
        let cmd = MitCommand {
            position_rad: 0.0,
            velocity_rad_s: 0.0,
            kp: 5000.0,
            kd: 100.0,
            torque_ff_nm: 0.0,
        };
        let (_id, payload) = codec.encode_mit(0, 1, cmd).unwrap();
        let dec = codec.decode_mit_payload(&payload);
        assert!((dec.kp - 5000.0).abs() < 0.2);
        assert!((dec.kd - 100.0).abs() < 0.02);
    }
}
