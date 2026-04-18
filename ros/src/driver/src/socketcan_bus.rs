//! Blocking SocketCAN wrapper (Linux only).
//!
//! This is **shared transport**: any future device family (RS01, RS02, RS04, sensors on CAN)
//! should use [`CanBus`] for raw frames—not fork a second socket wrapper.
//!
//! Non-Linux targets compile a stub so `cargo check` works on developer machines.

use std::io;
use std::time::Duration;

/// Linux SocketCAN extended-frame bit OR'd into `can_id` (`linux/can.h` `CAN_EFF_FLAG`).
///
/// Defined here (transport layer) so SocketCAN I/O does not depend on any device protocol module.
pub const CAN_EFF_FLAG: u32 = 0x8000_0000;

#[cfg(target_os = "linux")]
mod imp {
    use std::os::unix::io::AsRawFd;

    use super::io;
    use super::Duration;
    use embedded_can::{ExtendedId, Frame as EmbeddedFrame, Id};
    use socketcan::{CanFrame, CanSocket, Socket};

    /// Thin wrapper around a blocking CAN socket.
    #[derive(Debug)]
    pub struct CanBus {
        sock: CanSocket,
    }

    impl CanBus {
        pub fn open(ifname: &str) -> io::Result<Self> {
            let sock = CanSocket::open(ifname)?;
            let bus = Self { sock };
            if let Err(e) = bus.set_recv_own_msgs(false) {
                log::warn!(
                    "setsockopt(CAN_RAW_RECV_OWN_MSGS, 0) failed: {e}; continuing (may see own TX)"
                );
            }
            Ok(bus)
        }

        pub fn set_read_timeout(&self, d: Duration) -> io::Result<()> {
            self.sock.set_read_timeout(d)
        }

        /// When `false`, the kernel does not loop back our own TX frames (Linux `CAN_RAW_RECV_OWN_MSGS`).
        pub fn set_recv_own_msgs(&self, on: bool) -> io::Result<()> {
            let fd = self.sock.as_raw_fd();
            let v: libc::c_int = i32::from(on);
            let rc = unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_CAN_RAW,
                    libc::CAN_RAW_RECV_OWN_MSGS,
                    &v as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&v) as libc::socklen_t,
                )
            };
            if rc != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Send an extended data frame with 8-byte payload (`id` is 29-bit, EFF flag optional).
        pub fn send_ext(&self, id: u32, data: &[u8; 8]) -> io::Result<()> {
            let eid = ExtendedId::new(id & 0x1FFF_FFFF).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid extended CAN id")
            })?;
            let frame = CanFrame::new(eid, &data[..]).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "frame construction failed")
            })?;
            self.sock.write_frame(&frame)?;
            Ok(())
        }

        /// Blocking read of one frame. Returns full 32-bit `can_id` with `CAN_EFF_FLAG` set,
        /// up to 8 data bytes, and DLC.
        pub fn recv(&self) -> io::Result<(u32, [u8; 8], usize)> {
            let frame = self.sock.read_frame()?;
            if matches!(frame, CanFrame::Error(_)) {
                return Err(io::Error::other("received CAN error frame"));
            }
            let raw_29 = match EmbeddedFrame::id(&frame) {
                Id::Extended(eid) => eid.as_raw(),
                Id::Standard(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "expected extended CAN frame",
                    ));
                }
            };
            let can_id = raw_29 | super::CAN_EFF_FLAG;
            let mut data = [0u8; 8];
            let bytes = EmbeddedFrame::data(&frame);
            let dlc = bytes.len().min(8);
            data[..dlc].copy_from_slice(&bytes[..dlc]);
            Ok((can_id, data, dlc))
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::io;
    use super::Duration;

    #[derive(Debug)]
    pub struct CanBus;

    impl CanBus {
        pub fn open(_ifname: &str) -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "SocketCAN is only available on Linux targets",
            ))
        }

        pub fn set_read_timeout(&self, _d: Duration) -> io::Result<()> {
            Ok(())
        }

        pub fn set_recv_own_msgs(&self, _on: bool) -> io::Result<()> {
            Ok(())
        }

        pub fn send_ext(&self, _id: u32, _data: &[u8; 8]) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "SocketCAN is only available on Linux targets",
            ))
        }

        pub fn recv(&self) -> io::Result<(u32, [u8; 8], usize)> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "SocketCAN is only available on Linux targets",
            ))
        }
    }
}

pub use imp::CanBus;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_open_errors_on_non_linux() {
        #[cfg(not(target_os = "linux"))]
        {
            assert!(CanBus::open("vcan0").is_err());
        }
    }

    #[test]
    fn vcan_open_skipped_without_iface() {
        #[cfg(target_os = "linux")]
        {
            if let Ok(bus) = CanBus::open("vcan0") {
                let _ = bus.set_read_timeout(std::time::Duration::from_millis(1));
            }
        }
    }
}
