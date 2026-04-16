//! Rudy Robstride RS03 CAN driver building blocks.
//!
//! - [`protocol`]: encode/decode frames (public MIT-style layout; verify against firmware).
//! - [`socketcan_bus`]: thin SocketCAN send/recv wrapper.
//! - [`state_machine`]: actuator lifecycle for safe bring-up.

pub mod protocol;
pub mod socketcan_bus;
pub mod state_machine;

pub use protocol::{DecodedCommand, MitCommand, RobstrideCodec};
pub use socketcan_bus::CanBus;
pub use state_machine::{ActuatorEvent, ActuatorState, ActuatorStateMachine};
