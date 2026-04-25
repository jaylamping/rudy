//! Per-bus dedicated I/O thread.
//!
//! Each `[[can.buses]]` entry gets its own OS thread that exclusively owns
//! the underlying [`driver::CanBus`]. The thread runs a tight loop:
//!
//! 1. Block on `bus.recv()` for a short timeout (5 ms). Every received
//!    type-2 (`MotorFeedback`) frame is decoded and pushed into
//!    `state.latest` + `state.feedback_tx` immediately, so the live
//!    telemetry view tracks the bus at native cadence with no extra
//!    type-17 round-trips. Type-17 (`ReadParam`) replies are matched
//!    against in-flight commands by `(motor_id, index)` and forwarded to
//!    the originating thread via a oneshot.
//! 2. Drain up to N pending [`Cmd`]s from the per-bus channel. Each
//!    command serializes one or more frames on the bus (writes / enable /
//!    stop / read request) and, where the operation expects an
//!    acknowledgement, parks a oneshot in `pending` so the next received
//!    frame can complete it.
//!
//! Why a dedicated thread per bus, instead of the previous
//! per-iface mutex around `CanBus`? The mutex serialised every
//! `set_velocity_setpoint` against every `read_param`, which under the
//! 20 Hz sweep cadence used the entire bus budget on lock fights. A
//! single-owner thread serialises naturally with no lock, and the
//! `recv()`-first loop guarantees that a flood of type-2 frames is
//! drained continuously instead of starving while a slow `read_param`
//! waits on a missing peer.
//!
//! On the Pi 5 the worker pins itself to a CPU after spawn (see
//! [`spawn`]). Pinning + IRQ affinity (set by `deploy/pi5/bootstrap.sh`)
//! co-locates the SocketCAN softirq and the user-space recv loop on the
//! same core, which removes the inter-core hop on every frame.

#![cfg(target_os = "linux")]

mod command;
mod feedback;
mod handle;
mod health;
mod pin;
mod thread;

pub use command::{Cmd, ReplyBytes, WriteValue, REPLY_TIMEOUT};
pub use handle::{BusHandle, MitStreamSetpoint};
pub use health::BusHealth;
pub use pin::{auto_assign_cpu, available_cpus};
pub use thread::spawn;

#[cfg(test)]
#[path = "thread_tests.rs"]
mod tests;
