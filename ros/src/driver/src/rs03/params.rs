//! Parameter indices in the 0x70xx namespace (type-17 readable, type-18 writable RAM).

/// run_mode: uint8 0=MIT, 1=PP, 2=velocity, 3=current, 5=CSP
pub const RUN_MODE: u16 = 0x7005;
pub const IQ_REF: u16 = 0x7006;
pub const SPD_REF: u16 = 0x700A;
pub const LIMIT_TORQUE: u16 = 0x700B;
pub const LOC_REF: u16 = 0x7016;
pub const LIMIT_SPD: u16 = 0x7017;
pub const LIMIT_CUR: u16 = 0x7018;
pub const MECH_POS: u16 = 0x7019;
pub const IQF: u16 = 0x701A;
pub const MECH_VEL: u16 = 0x701B;
pub const VBUS: u16 = 0x701C;
pub const ACC_RAD: u16 = 0x7022;
pub const VEL_MAX: u16 = 0x7024;
pub const ACC_SET: u16 = 0x7025;
pub const EPSCAN_TIME: u16 = 0x7026;
pub const CAN_TIMEOUT: u16 = 0x7028;
pub const ZERO_STA: u16 = 0x7029;
pub const DAMPER: u16 = 0x702A;
pub const ADD_OFFSET: u16 = 0x702B;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type18_write_float_payload_matches_python_pack() {
        let index = SPD_REF;
        let value = 1.5_f32;
        let mut payload = [0u8; 8];
        payload[0..2].copy_from_slice(&index.to_le_bytes());
        payload[4..8].copy_from_slice(&value.to_le_bytes());
        assert_eq!(payload, [0x0A, 0x70, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x3F]);
    }

    #[test]
    fn type18_write_u8_payload_matches_python_pack() {
        let mut payload = [0u8; 8];
        payload[0..2].copy_from_slice(&RUN_MODE.to_le_bytes());
        payload[4] = 2;
        assert_eq!(payload, [0x05, 0x70, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00]);
    }
}
