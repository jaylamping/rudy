//! Server-side closed-loop motion controllers.
//!
//! # Convention going forward
//!
//! **Any closed-loop motion that the daemon can run autonomously MUST
//! run on the daemon. The browser expresses *intent* and observes
//! *status*; it does NOT drive the loop.**
//!
//! Concretely: the SPA should never `setInterval` an HTTP POST to
//! `/jog` (or any future per-frame motion endpoint) to synthesize a
//! sweep / wave / continuous-jog pattern. Such patterns belong here in
//! `motion::`, behind a single REST `start` + `stop` (and, for
//! operator-finger dead-man patterns, a WebTransport bidi stream that
//! the controller treats as the dead-man signal).
//!
//! Why this rule exists: the SPA-driven version produces visible jitter
//! every time HTTP latency exceeds the daemon's TTL watchdog window, at
//! which point `cmd_stop` fires and the next request restarts the
//! velocity loop with a fresh `RUN_MODE + cmd_enable + SPD_REF` re-arm.
//! See the "Server-side motion controllers" plan and the original
//! "Sweep-safe CAN I/O" plan for the disaster history.
//!
//! # Module layout
//!
//! * [`intent`] — `MotionIntent` / `MotionStatus` wire types.
//! * [`preflight`] — shared safety preflight extracted from
//!   [`crate::api::jog`]. Every motion entry point runs the same checks.
//! * [`sweep`] / [`wave`] / per-pattern step functions (pure; no IO).
//! * [`controller`] — the per-motor long-running tokio task.
//! * [`registry`] — owns the active controller per role; routes
//!   start / stop / intent updates.
//!
//! # Adding a new pattern
//!
//! 1. Add a variant to [`intent::MotionIntent`] carrying only the
//!    parameters the per-pattern step function needs.
//! 2. Add `motion::<your_pattern>::step(...)` as a pure function.
//! 3. Hook it into the `match intent` arm in
//!    [`controller::run`].
//! 4. Add `POST /motors/:role/motion/<your_pattern>` to
//!    [`crate::api::motion`].
//! 5. Add a [`controller::run`] unit test that drives the new arm
//!    through a few ticks against a synthetic feedback row.
//!
//! Most patterns are <100 lines including tests because the ugly
//! parts (preflight, cancellation, audit, status broadcast) are
//! shared.

pub mod controller;
pub mod intent;
pub mod preflight;
pub mod registry;
pub mod sweep;
pub mod wave;

pub use intent::{MotionIntent, MotionState, MotionStatus, MotionStopReason};
pub use preflight::{PreflightChecks, PreflightFailure, PreflightOk};
pub use registry::{MotionRegistry, MotionSnapshot};
