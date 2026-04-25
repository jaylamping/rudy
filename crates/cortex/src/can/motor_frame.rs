//! Logical ↔ firmware scalar sign at RS03 CAN boundary.
//!
//! Inventory `direction_sign` maps logical joint frame (positive command ⇒
//! positive measured motion) to firmware-reported scalars.

#[inline]
pub fn logical_scalar_from_firmware(firmware: f32, direction_sign: f32) -> f32 {
    firmware * direction_sign
}

#[inline]
pub fn firmware_scalar_from_logical(logical: f32, direction_sign: f32) -> f32 {
    logical * direction_sign
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_plus_identity() {
        assert!((logical_scalar_from_firmware(1.25, 1.0) - 1.25).abs() < 1e-6);
        assert!((firmware_scalar_from_logical(1.25, 1.0) - 1.25).abs() < 1e-6);
    }

    #[test]
    fn sign_minus_inverts() {
        assert!((logical_scalar_from_firmware(2.0, -1.0) - (-2.0)).abs() < 1e-6);
        assert!((firmware_scalar_from_logical(-3.0, -1.0) - 3.0).abs() < 1e-6);
    }
}
