//! Client-to-server WebTransport frames.
//!
//! The server-to-client side of the WT protocol is described by
//! `declare_wt_streams!` and rides datagrams (or the lazily-opened reliable
//! uni-stream) inside the [`crate::types::WtEnvelope`] envelope. The
//! client-to-server side is this enum: every CBOR doc the SPA writes onto
//! a bidirectional QUIC stream is a [`ClientFrame`].
//!
//! Why bidirectional streams (rather than POST):
//!
//! * Hold-to-jog wants ~5 Hz heartbeats; an HTTP round-trip per heartbeat
//!   adds tens of milliseconds of latency *and* burns a TCP socket per
//!   request. A single QUIC bidi stream amortizes the handshake and gets
//!   us the dead-man semantics for free (peer disconnect ≡ stream EOF ≡
//!   `MotionStopReason::ClientGone`).
//! * The same transport pattern will plug straight into future
//!   tele-operation surfaces (joystick, MoCap retarget, scripted
//!   sequencer) without re-litigating "should this be REST or WS or WT?"
//!   for every new dial.
//!
//! # Wire format
//!
//! Each bidi stream carries a sequence of length-prefixed CBOR docs:
//!
//! ```text
//! frame = u32 BE length | CBOR body (ClientFrame)
//! ```
//!
//! The length prefix matches the server-to-client reliable-stream framing
//! in [`crate::wt_router::SessionRouter::write_reliable`], so the SPA's
//! framer is symmetric. A frame with `length == 0` is reserved (and
//! currently rejected) so future "ping" semantics don't collide with the
//! existing decoders.
//!
//! # Lifetimes & safety
//!
//! A motion-bearing bidi stream owns the role it's driving for the
//! lifetime of the stream. The router stops the role's motion if the
//! stream EOFs without a preceding `MotionStop` (see
//! [`crate::wt_router::run_client_stream`]). `MotionStop` is idempotent
//! and does not require the same stream that started the motion — any
//! stream may stop any role.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::types::WtSubscribe;

/// Anything the SPA can send the daemon over a WebTransport bidi stream.
///
/// # Adding a variant
///
/// 1. Add the variant + ts-rs export here.
/// 2. Add a dispatch arm in
///    [`crate::wt_router::dispatch_client_frame`].
/// 3. Add a SPA helper in `link/src/lib/wt/clientStream.ts`.
///
/// Variant names use snake_case in CBOR (matches every other client-side
/// enum on the wire). The SPA's `clientStream.ts` is the single source of
/// truth for the encoding and is auto-generated against the ts-rs export
/// of this enum.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClientFrame {
    /// Replace the per-session subscription filter. Equivalent to the
    /// legacy "open a bidi, write one CBOR doc, close" path; the router
    /// dispatches both shapes through the same handler.
    Subscribe(WtSubscribe),

    /// Start (or hot-swap the velocity of) a server-side jog on `role`.
    /// The first `MotionJog` for a role spawns the controller; subsequent
    /// frames update the live velocity setpoint *and* refresh the
    /// dead-man heartbeat.
    ///
    /// `vel_rad_s` is clamped server-side to `MAX_MOTION_VEL_RAD_S`
    /// (see [`crate::api::motion`]). The clamp is silent so the SPA
    /// can ship a slider value as-is without pre-clamping.
    MotionJog { role: String, vel_rad_s: f32 },

    /// Refresh the jog dead-man window without changing velocity. The
    /// controller treats this exactly like a re-send of the most recent
    /// [`ClientFrame::MotionJog`] but avoids re-encoding velocity when
    /// the operator's finger isn't moving the slider. Sending one every
    /// 200 ms is plenty for the default 250 ms heartbeat TTL.
    MotionHeartbeat { role: String },

    /// Stop any active motion for `role`. Idempotent; safe to send from
    /// a stream that didn't start the motion.
    MotionStop { role: String },
}

#[cfg(test)]
#[path = "client_frames_tests.rs"]
mod tests;
