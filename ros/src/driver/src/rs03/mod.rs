//! RobStride **RS03** CAN protocol (ADR-0002).
//!
//! This module is the namespaced home for everything that is **specific to RS03**
//! framing and semantics. It keeps the crate root free for shared infrastructure
//! ([crate::socketcan_bus], [crate::state_machine]) and future device families
//! (RS01, RS02, RS04, other buses, sensors) as **sibling modules** under src/,
//! e.g. crate::rs02, without type-name collisions (CommType, params, …).
//!
//! ## Layout
//!
//! | Submodule | Role |
//! |-----------|------|
//! | [comm_types] | Comm type enum (CAN ID high bits) |
//! | [frame] | 29-bit arbitration ID helpers |
//! | [params] | 0x70xx register indices |
//! | [mit] | MIT / operation-control payload codec |
//! | [feedback] | Type-2 motor feedback decode |
//! | [errors] | RS03 encode/decode / reply errors |
//! | [session] | Blocking I/O helpers on [crate::socketcan_bus::CanBus] |
//! | [tests] | Reusable bench / commissioning routines (`read`, `set_zero`, `smoke`, `jog`) |
//! | [actuator] | [Rs03] instance implementing [crate::robstride::RsActuator] |
//!
//! New RSxx lines should follow the same split: **transport stays generic**,
//! **protocol stays per-family** in its own directory tree, each with a concrete type that
//! implements [crate::robstride::RsActuator].

pub mod actuator;
pub mod comm_types;
pub mod errors;
pub mod feedback;
pub mod frame;
pub mod mit;
pub mod params;
pub mod session;
pub mod tests;

pub use actuator::Rs03;
pub use comm_types::CommType;
pub use errors::ProtocolError;
pub use feedback::{decode_motor_feedback, MotorFeedback};
pub use frame::{
    arb_id, comm_type_from_id, passive_observer_node_id, strip_eff_flag, with_eff_flag, CAN_EFF_FLAG,
};
pub use mit::{DecodedCommand, MitCommand, RobstrideCodec};
