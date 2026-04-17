//! RS03 protocol errors (encode/decode and reply semantics).
//!
//! Other device families should define their own error enums under `crate::<family>/errors.rs`
//! rather than extending this type—keeps `match` exhaustiveness local to one protocol.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("value out of representable range")]
    OutOfRange,
    #[error("CAN frame data too short (got {got}, need {need})")]
    ShortFrame { got: usize, need: usize },
    #[error(
        "motor rejected read of index 0x{index:04X} (reply status 0x{status:02X}); index may be outside the 0x70xx type-17 list"
    )]
    ReadRejected { index: u16, status: u8 },
    #[error("reply index mismatch (expected 0x{expected:04X}, got 0x{got:04X})")]
    ReplyIndexMismatch { expected: u16, got: u16 },
    #[error("invalid communication type in frame: 0x{0:02X}")]
    InvalidCommType(u8),
}
