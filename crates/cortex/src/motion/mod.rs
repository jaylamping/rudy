//! Server-side closed-loop motion controllers.
//!
//! # Convention going forward
//!
//! **Any closed-loop motion that the daemon can run autonomously MUST
//! run on the daemon. The browser expresses *intent* and observes
//! *status*; it does NOT drive the loop.**
//!
//! # Module layout
//!
//! * [`intent`] — [`MotionIntent`] wire type.
//! * [`status`] — [`MotionStatus`] / [`MotionState`] / [`MotionStopReason`].
//! * [`preflight`] — shared safety preflight extracted from
//!   [`crate::api::jog`]. Every motion entry point runs the same checks.
//! * [`patterns`] — [`patterns::sweep`] / [`patterns::wave`] and future
//!   per-pattern step functions (pure; no IO).
//! * [`controller`] — the per-motor long-running tokio task.
//! * [`registry`] — owns the active controller per role; routes
//!   start / stop / intent updates.
//!
//! # Adding a new pattern
//!
//! 1. Add a variant to [`intent::MotionIntent`] carrying only the
//!    parameters the per-pattern step function needs.
//! 2. Add `motion::patterns::<your_pattern>::step(...)` as a pure function.
//! 3. Hook it into the `match intent` arm in [`controller::run`].
//! 4. Add `POST /motors/:role/motion/<your_pattern>` to
//!    [`crate::api::motion`].
//! 5. Add a [`controller::run`] unit test that drives the new arm
//!    through a few ticks against a synthetic feedback row.

pub mod controller;
pub mod intent;
pub mod mit;
pub mod patterns;
pub mod preflight;
pub mod registry;
pub mod smoothing;
pub mod status;
pub mod tick;

pub use intent::{
    default_turnaround_rad, MotionIntent, OVERSHOOT_S, SWEEP_BASE_INSET_RAD, WAVE_BASE_INSET_RAD,
};
pub use patterns::{sweep, wave};
pub use preflight::{PreflightChecks, PreflightFailure, PreflightOk};
pub use registry::{MotionRegistry, MotionSnapshot};
pub use status::{
    classify_motion_bus_string, MotionBusError, MotionState, MotionStatus, MotionStopReason,
};
pub use tick::motion_tick_interval_ms;

#[cfg(test)]
#[path = "motion_tests.rs"]
mod motion_tests;
