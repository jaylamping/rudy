//! Blocking SocketCAN wrapper for Rudy driver (Linux only).
//!
//! Non-Linux targets compile a stub so `cargo check` works on macOS dev machines.

use std::io;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod imp {
    use super::io;
    use super::Duration;
    use embedded_can::ExtendedId;
    use socketcan::blocking::CanSocket;
    use socketcan::{CanFrame, Frame, Socket};

    /// Thin wrapper around a blocking CAN socket.
    #[derive(Debug)]
    pub struct CanBus {
        sock: CanSocket,
    }

    impl CanBus {
        pub fn open(ifname: &str) -> io::Result<Self> {
            let sock = CanSocket::open(ifname)?;
            Ok(Self { sock })
        }

        pub fn set_read_timeout(&self, d: Duration) -> io::Result<()> {
            self.sock.set_read_timeout(d)
        }

        /// Send an extended data frame with 8-byte payload.
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

        pub fn send_ext(&self, _id: u32, _data: &[u8; 8]) -> io::Result<()> {
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
