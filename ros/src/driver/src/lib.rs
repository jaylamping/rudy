//! Rudy low-level drivers and device protocols.
//!
//! # Crate layout (extensibility)
//!
//! - **[`socketcan_bus`]** — OS-facing CAN transport (SocketCAN). **Shared** by every
//!   device family that uses this stack; keep it free of RS03-only constants.
//! - **[`state_machine`]** — Generic actuator lifecycle states/events. **Family-agnostic**;
//!   wire it to RS03 (or other) sessions at the node / orchestration layer.
//! - **[`robstride`]** — Shared **RSxx** abstraction: [`robstride::RsActuator`] (sealed trait) and
//!   [`robstride::RsModel`]. Each line is a concrete type ([`rs03::Rs03`], future `Rs02`, …)
//!   implementing the trait—analogous to a base class in OOP.
//! - **[`rs03`]** — RobStride **RS03** protocol: IDs, parameters, MIT codec, blocking session
//!   helpers ([`rs03::session`]), and the [`rs03::Rs03`] actuator handle. Spec: ADR-0002.
//!   Additional actuators should get sibling modules (`rs02`, …) rather than extending `rs03`.
//!
//! Prefer **`driver::rs03::…`** for codecs and **`driver::robstride::RsActuator`** for polymorphic
//! orchestration. Items are also re-exported at the crate root for existing call sites.

pub mod robstride;
pub mod rs03;
pub mod socketcan_bus;
pub mod state_machine;

pub use robstride::{RsActuator, RsModel};
pub use rs03::params;
pub use rs03::session;
pub use rs03::{
    arb_id, comm_type_from_id, decode_motor_feedback, passive_observer_node_id, strip_eff_flag,
    with_eff_flag, CommType, DecodedCommand, MitCommand, MotorFeedback, ProtocolError,
    RobstrideCodec, Rs03, CAN_EFF_FLAG,
};
pub use socketcan_bus::CanBus;
pub use state_machine::{ActuatorEvent, ActuatorState, ActuatorStateMachine};
