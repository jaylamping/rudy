//! Type-17/18 four-byte payload helpers for RS03 RAM registers (ADR-0002).

/// Build the 8-byte type-18 write payload: `index` LE in bytes 0–1, zero pad
/// 2–3, value LE in bytes 4–7.
#[inline]
pub fn type18_payload_f32(index: u16, value: f32) -> [u8; 8] {
    let mut p = [0u8; 8];
    p[0..2].copy_from_slice(&index.to_le_bytes());
    p[4..8].copy_from_slice(&value.to_le_bytes());
    p
}

/// Same layout as [`type18_payload_f32`] but for a single-byte scalar in byte 4.
#[inline]
pub fn type18_payload_u8(index: u16, value: u8) -> [u8; 8] {
    let mut p = [0u8; 8];
    p[0..2].copy_from_slice(&index.to_le_bytes());
    p[4] = value;
    p
}

/// Full dword payload (e.g. `uint32` / `uint16` widened to u32 on the wire).
#[inline]
pub fn type18_payload_u32(index: u16, value: u32) -> [u8; 8] {
    let mut p = [0u8; 8];
    p[0..2].copy_from_slice(&index.to_le_bytes());
    p[4..8].copy_from_slice(&value.to_le_bytes());
    p
}

/// Decode the value dword from a successful type-17 reply (`data[4..8]`).
#[inline]
pub fn decode_type17_value_f32(data: &[u8; 8]) -> f32 {
    f32::from_le_bytes([data[4], data[5], data[6], data[7]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type18_f32_matches_python_reference() {
        let index = 0x700A_u16;
        let value = 1.5_f32;
        let p = type18_payload_f32(index, value);
        assert_eq!(p, [0x0A, 0x70, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x3F]);
    }

    #[test]
    fn type18_u8_run_mode_matches_python_reference() {
        let p = type18_payload_u8(0x7005, 2);
        assert_eq!(p, [0x05, 0x70, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00]);
    }
}
