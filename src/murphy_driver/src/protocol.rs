//! Robstride-style CAN frame codec (extended ID, 8-byte payload).
//!
//! This implements a **self-consistent** MIT-style command packing layout used for unit tests
//! and as a starting point for firmware bring-up. **Verify every field** against your actuator docs.

use thiserror::Error;

const P_MIN: f32 = -12.5;
const P_MAX: f32 = 12.5;
const V_MIN: f32 = -30.0;
const V_MAX: f32 = 30.0;
const KP_MIN: f32 = 0.0;
const KP_MAX: f32 = 500.0;
const KD_MIN: f32 = 0.0;
const KD_MAX: f32 = 5.0;
const T_MIN: f32 = -60.0;
const T_MAX: f32 = 60.0;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CodecError {
    #[error("value out of representable range")]
    OutOfRange,
}

fn float_to_uint(x: f32, x_min: f32, x_max: f32, bits: u8) -> Result<u16, CodecError> {
    if x < x_min || x > x_max {
        return Err(CodecError::OutOfRange);
    }
    let span = x_max - x_min;
    let norm = (x - x_min) / span;
    let max_u = ((1u32 << bits) - 1) as f32;
    Ok((norm * max_u) as u16)
}

fn uint_to_float(x: u16, x_min: f32, x_max: f32, bits: u8) -> f32 {
    let span = x_max - x_min;
    let max_u = ((1u32 << bits) - 1) as f32;
    x_min + (x as f32) * span / max_u
}

/// MIT-style command parameters (vendor mode 0 style).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MitCommand {
    pub position_rad: f32,
    pub velocity_rad_s: f32,
    pub kp: f32,
    pub kd: f32,
    pub torque_ff_nm: f32,
}

/// Decoded command parameters (inverse of [`RobstrideCodec::encode_mit`] packing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecodedCommand {
    pub position_rad: f32,
    pub velocity_rad_s: f32,
    pub kp: f32,
    pub kd: f32,
    pub torque_ff_nm: f32,
}

/// Encode/decode helper.
#[derive(Debug, Default, Clone, Copy)]
pub struct RobstrideCodec;

impl RobstrideCodec {
    /// Build extended CAN ID: `0x0000_0C00 | (motor_id & 0xFF) << 5`.
    pub fn command_id(motor_id: u8) -> u32 {
        0x0000_0C00u32 | ((motor_id as u32) & 0xFF) << 5
    }

    pub fn encode_mit(&self, motor_id: u8, cmd: MitCommand) -> Result<(u32, [u8; 8]), CodecError> {
        let p = float_to_uint(cmd.position_rad, P_MIN, P_MAX, 16)?;
        let v = float_to_uint(cmd.velocity_rad_s, V_MIN, V_MAX, 12)?;
        let kp = float_to_uint(cmd.kp, KP_MIN, KP_MAX, 12)?;
        let kd = float_to_uint(cmd.kd, KD_MIN, KD_MAX, 12)?;
        let t = float_to_uint(cmd.torque_ff_nm, T_MIN, T_MAX, 12)?;

        let mut data = [0u8; 8];
        data[0] = (p >> 8) as u8;
        data[1] = p as u8;
        data[2] = (v >> 4) as u8;
        data[3] = (((v & 0xF) << 4) | (kp >> 8)) as u8;
        data[4] = kp as u8;
        data[5] = (kd >> 4) as u8;
        data[6] = (((kd & 0xF) << 4) | (t >> 8)) as u8;
        data[7] = t as u8;

        Ok((Self::command_id(motor_id), data))
    }

    /// Decode the 8-byte MIT command payload produced by [`Self::encode_mit`].
    pub fn decode_mit_command(&self, data: &[u8; 8]) -> DecodedCommand {
        let p = u16::from_be_bytes([data[0], data[1]]);
        let v = (((data[2] as u16) << 4) | ((data[3] >> 4) as u16)) & 0x0FFF;
        let kp = ((((data[3] & 0x0F) as u16) << 8) | data[4] as u16) & 0x0FFF;
        let kd = (((data[5] as u16) << 4) | ((data[6] >> 4) as u16)) & 0x0FFF;
        let t = ((((data[6] & 0x0F) as u16) << 8) | data[7] as u16) & 0x0FFF;

        DecodedCommand {
            position_rad: uint_to_float(p, P_MIN, P_MAX, 16),
            velocity_rad_s: uint_to_float(v, V_MIN, V_MAX, 12),
            kp: uint_to_float(kp, KP_MIN, KP_MAX, 12),
            kd: uint_to_float(kd, KD_MIN, KD_MAX, 12),
            torque_ff_nm: uint_to_float(t, T_MIN, T_MAX, 12),
        }
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
        let (id, payload) = codec.encode_mit(3, cmd).unwrap();
        assert_eq!(id, RobstrideCodec::command_id(3));
        let dec = codec.decode_mit_command(&payload);
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
        assert_eq!(codec.encode_mit(1, cmd), Err(CodecError::OutOfRange));
    }
}
