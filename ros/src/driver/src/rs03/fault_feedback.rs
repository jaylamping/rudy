//! Type-0x15 fault-feedback frame (ADR-0002, vendor manual §3.3.7).

use super::errors::ProtocolError;

/// Decoded fault dword pair from an 8-byte fault-feedback payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultDwords {
    pub fault: u32,
    pub warn: u32,
}

/// Parse `data` as two little-endian `u32` fault / warning registers.
pub fn decode_fault_dwords(data: &[u8]) -> Result<FaultDwords, ProtocolError> {
    if data.len() < 8 {
        return Err(ProtocolError::ShortFrame {
            got: data.len(),
            need: 8,
        });
    }
    Ok(FaultDwords {
        fault: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        warn: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_roundtrip_bytes() {
        let mut d = [0u8; 8];
        d[0..4].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        d[4..8].copy_from_slice(&0xAABB_CCDDu32.to_le_bytes());
        let f = decode_fault_dwords(&d).unwrap();
        assert_eq!(f.fault, 0x0102_0304);
        assert_eq!(f.warn, 0xAABB_CCDD);
    }
}
