//! Linux SocketCAN core - real hardware path.
//!
//! Compile-gated placeholder. The wiring to driver::socketcan_bus::CanBus
//! and driver::rs03::session lives here and replaces the mock path when
//! cfg.can.mock = false on Linux.

#![cfg(target_os = "linux")]

#[allow(non_upper_case_globals)]
pub const placeholder: () = ();
