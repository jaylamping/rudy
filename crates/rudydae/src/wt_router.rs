//! Per-session WebTransport stream router.
//!
//! Owns the small amount of state that's specific to one connected client:
//! the active subscription filter, per-stream sequence counters, and one
//! handle to each broadcast channel the session is currently listening on.
//!
//! Why a router exists at all (vs. inlining a `tokio::select!` in `wt.rs`):
//!
//! 1. Adding a stream shouldn't require editing `wt::handle_session`. The
//!    macro in `types.rs` plus a `register_<kind>` call here is the entire
//!    shopping list — the session loop is generic across all streams.
//! 2. The reliability tier (`WtTransport::Datagram` vs `Stream`) is decided
//!    per stream at compile time, but the *delivery mechanism* is decided
//!    per session at runtime (the same `Fault` payload always rides a
//!    reliable stream, but the stream itself is opened lazily on first frame
//!    and shared across kinds). The router is the natural place for that
//!    bookkeeping.
//! 3. Per-stream sequence numbers must live somewhere that survives
//!    `recv_lagged` resets but resets when the session reconnects. Per-router
//!    is the right scope.
//!
//! What the router does NOT do:
//! - Open the QUIC connection (that's `wt::handle_session`).
//! - Parse `WtSubscribe` (that's `wt::handle_session`, which feeds the
//!   parsed value into `Router::apply_subscribe`).
//! - Fan-in from arbitrary mpsc sources — every source must be a
//!   `broadcast::Sender` registered on `AppState`. This keeps the daemon's
//!   "one writer, many readers" model honest.

use std::collections::HashMap;

use anyhow::Result;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, trace, warn};
use wtransport::Connection;
use wtransport::SendStream;

use crate::state::SharedState;
use crate::types::{
    MotorFeedback, SystemSnapshot, WtEnvelope, WtKind, WtPayload, WtSubscribe,
    WtSubscribeFilters, WtTransport,
};

/// Per-session router: owns subscriptions, sequence counters, the lazily-
/// opened reliable stream handle, and a CBOR scratch buffer reused across
/// frames.
pub struct SessionRouter {
    /// Filter state. Mutable: `apply_subscribe` rewrites it on the bidi
    /// stream message arrival. Default = "everything".
    filter: SubscriptionFilter,

    /// Per-kind monotonically-increasing sequence. Allocated lazily on
    /// first send; reset on reconnect (a new `SessionRouter` is built per
    /// session).
    seq: HashMap<WtKind, u64>,

    /// Reliable QUIC uni-stream, opened on first reliable frame. `None`
    /// until then; a peer that never receives a reliable frame never sees
    /// a stream.
    reliable_stream: Option<SendStream>,

    /// Reusable CBOR scratch buffer. Sized for the largest expected
    /// payload (a SystemSnapshot is ~200 bytes today). The router drains
    /// the buffer before each encode, so a one-time allocation suffices.
    scratch: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
struct SubscriptionFilter {
    /// `None` ≡ "all kinds" (default at session open). `Some(set)` is the
    /// explicit narrow filter from the most recent `WtSubscribe`.
    kinds: Option<Vec<WtKind>>,
    /// Per-kind sub-filters. Default-empty struct ≡ no narrowing.
    sub: WtSubscribeFilters,
}

impl SubscriptionFilter {
    fn allows_kind(&self, kind: WtKind) -> bool {
        match &self.kinds {
            None => true,
            Some(list) => list.contains(&kind),
        }
    }

    /// Per-payload narrow predicate. Returns `false` to drop the frame.
    /// Adding a new sub-filter dimension means: extend
    /// `WtSubscribeFilters`, then add a branch here. The router stays
    /// otherwise unchanged.
    fn allows_motor(&self, role: &str) -> bool {
        let roles = &self.sub.motor_roles;
        roles.is_empty() || roles.iter().any(|r| r == role)
    }
}

impl Default for SessionRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionRouter {
    pub fn new() -> Self {
        Self {
            filter: SubscriptionFilter::default(),
            seq: HashMap::new(),
            reliable_stream: None,
            scratch: Vec::with_capacity(256),
        }
    }

    /// Replace the active filter with one parsed from a client `WtSubscribe`
    /// message. Empty `kinds` ≡ "all" (matches the default behavior).
    pub fn apply_subscribe(&mut self, sub: WtSubscribe) {
        self.filter = SubscriptionFilter {
            kinds: if sub.kinds.is_empty() {
                None
            } else {
                Some(sub.kinds)
            },
            sub: sub.filters,
        };
        debug!("wt: applied subscription filter {:?}", self.filter);
    }

    /// Should this kind+payload pair be forwarded to the peer?
    /// Cheap; called per frame on the hot path.
    pub fn allows_motor_feedback(&self, fb: &MotorFeedback) -> bool {
        self.filter.allows_kind(WtKind::MotorFeedback) && self.filter.allows_motor(&fb.role)
    }

    pub fn allows_system_snapshot(&self) -> bool {
        self.filter.allows_kind(WtKind::SystemSnapshot)
    }

    /// Encode `payload` in a `WtEnvelope`, allocate the next sequence
    /// number, and dispatch to the right transport. Returns `Err` only
    /// for QUIC-level failures (peer disconnect, stream reset). CBOR
    /// encode failures are logged + swallowed because there is nothing
    /// useful the caller can do — the next frame is the recovery.
    pub async fn send<T: WtPayload + Serialize>(
        &mut self,
        connection: &Connection,
        kind: WtKind,
        payload: T,
    ) -> Result<()> {
        let seq = {
            let entry = self.seq.entry(kind).or_insert(0);
            let s = *entry;
            *entry = entry.wrapping_add(1);
            s
        };

        let envelope = WtEnvelope::new(seq, payload);
        self.scratch.clear();
        if let Err(e) = ciborium::into_writer(&envelope, &mut self.scratch) {
            warn!("wt: cbor encode failed for kind={}: {e:#}", kind.as_str());
            return Ok(());
        }

        match T::TRANSPORT {
            WtTransport::Datagram => {
                // send_datagram is sync (no await). On overflow / closed it
                // returns Err; we propagate so the session loop can break.
                connection
                    .send_datagram(self.scratch.as_slice())
                    .map_err(|e| anyhow::anyhow!("send_datagram failed: {e}"))?;
                trace!("wt: dgram kind={} seq={} bytes={}", kind.as_str(), seq, self.scratch.len());
            }
            WtTransport::Stream => {
                self.write_reliable(connection).await?;
                trace!(
                    "wt: stream kind={} seq={} bytes={}",
                    kind.as_str(),
                    seq,
                    self.scratch.len()
                );
            }
        }

        Ok(())
    }

    /// Write the current `scratch` buffer to the lazily-opened reliable
    /// uni-stream as a length-prefixed frame: `u32 BE length | bytes`.
    /// Length-prefixing is necessary because QUIC streams are byte streams
    /// — without an envelope-level frame boundary the reader can't know
    /// where one CBOR doc ends and the next begins.
    async fn write_reliable(&mut self, connection: &Connection) -> Result<()> {
        let stream = match &mut self.reliable_stream {
            Some(s) => s,
            None => {
                let new = connection
                    .open_uni()
                    .await
                    .map_err(|e| anyhow::anyhow!("open_uni failed: {e}"))?
                    .await
                    .map_err(|e| anyhow::anyhow!("uni-stream init failed: {e}"))?;
                self.reliable_stream = Some(new);
                self.reliable_stream.as_mut().expect("just inserted")
            }
        };

        let len = u32::try_from(self.scratch.len())
            .map_err(|_| anyhow::anyhow!("reliable frame too large: {}", self.scratch.len()))?;
        let mut header = [0u8; 4];
        header.copy_from_slice(&len.to_be_bytes());

        stream
            .write_all(&header)
            .await
            .map_err(|e| anyhow::anyhow!("reliable header write: {e}"))?;
        stream
            .write_all(&self.scratch)
            .await
            .map_err(|e| anyhow::anyhow!("reliable body write: {e}"))?;
        Ok(())
    }
}

/// Drive one session to completion. Runs until the connection drops, the
/// peer cancels, or any underlying channel yields a fatal error.
///
/// This function is generic across registered streams via a hand-coded
/// dispatch over the `WtKind` enum. Adding a stream means: register a
/// payload + macro entry in `types.rs`, add a `broadcast::Sender` to
/// `AppState`, then add one `recv()` arm here. The arm shape is identical
/// across kinds (subscribe, recv, allows_*, send), so a future macro can
/// fold it away if the count grows.
pub async fn run_session(connection: Connection, state: SharedState) -> Result<()> {
    let mut router = SessionRouter::new();
    let mut feedback_rx = state.feedback_tx.subscribe();
    let mut system_rx = state.system_tx.subscribe();

    // Concurrent tasks: the WT session can simultaneously
    //   (a) accept a bidi stream carrying one `WtSubscribe`, and
    //   (b) push frames to the peer.
    // We share `&mut router` across these via a single-tasking select loop.
    let mut subscribe_fut = Box::pin(read_subscribe(&connection));

    loop {
        tokio::select! {
            // (a) Subscription updates from the client.
            res = &mut subscribe_fut => {
                match res {
                    Ok(Some(sub)) => {
                        router.apply_subscribe(sub);
                        // Re-arm: a client may renegotiate at any time by
                        // opening a new bidi stream with a new `WtSubscribe`.
                        subscribe_fut = Box::pin(read_subscribe(&connection));
                    }
                    Ok(None) => {
                        // Peer closed the bidi stream without sending; arm
                        // for the next attempt.
                        subscribe_fut = Box::pin(read_subscribe(&connection));
                    }
                    Err(e) => {
                        debug!("wt: subscribe stream error (continuing with current filter): {e:#}");
                        subscribe_fut = Box::pin(read_subscribe(&connection));
                    }
                }
            }

            // (b1) Motor feedback fan-out.
            res = feedback_rx.recv() => match res {
                Ok(fb) => {
                    if router.allows_motor_feedback(&fb) {
                        if let Err(e) = router.send::<MotorFeedback>(&connection, WtKind::MotorFeedback, fb).await {
                            debug!("wt: motor_feedback send failed; closing session: {e:#}");
                            break;
                        }
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    debug!("wt: feedback receiver lagged {n}");
                }
                Err(RecvError::Closed) => break,
            },

            // (b2) System snapshot fan-out.
            res = system_rx.recv() => match res {
                Ok(snap) => {
                    if router.allows_system_snapshot() {
                        if let Err(e) = router.send::<SystemSnapshot>(&connection, WtKind::SystemSnapshot, snap).await {
                            debug!("wt: system_snapshot send failed; closing session: {e:#}");
                            break;
                        }
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    debug!("wt: system receiver lagged {n}");
                }
                Err(RecvError::Closed) => break,
            },
        }
    }

    Ok(())
}

/// Wait for the client to open a bidi stream and send one `WtSubscribe`
/// (CBOR). Returns `Ok(None)` if the stream closes with no body (peer just
/// wants the default "everything").
async fn read_subscribe(connection: &Connection) -> Result<Option<WtSubscribe>> {
    let (mut _send, mut recv) = connection
        .accept_bi()
        .await
        .map_err(|e| anyhow::anyhow!("accept_bi failed: {e}"))?;

    // Bound the read so a malicious client can't OOM us with a giant
    // pseudo-subscribe. WtSubscribe is small (kinds + tiny filter struct);
    // 8 KiB is generous.
    const MAX_SUBSCRIBE_BYTES: usize = 8 * 1024;
    let mut buf = Vec::with_capacity(256);
    let mut chunk = [0u8; 1024];
    loop {
        match recv.read(&mut chunk).await {
            Ok(Some(n)) if n > 0 => {
                if buf.len() + n > MAX_SUBSCRIBE_BYTES {
                    return Err(anyhow::anyhow!(
                        "WtSubscribe payload exceeds {MAX_SUBSCRIBE_BYTES} bytes"
                    ));
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Ok(_) => break, // None or 0 -> stream finished
            Err(e) => return Err(anyhow::anyhow!("subscribe stream read: {e}")),
        }
    }

    if buf.is_empty() {
        return Ok(None);
    }
    let sub: WtSubscribe = ciborium::de::from_reader(buf.as_slice())
        .map_err(|e| anyhow::anyhow!("WtSubscribe cbor decode: {e}"))?;
    Ok(Some(sub))
}
