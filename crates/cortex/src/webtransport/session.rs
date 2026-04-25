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
use std::sync::{Arc, RwLock};

use anyhow::Result;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, trace, warn};
use wtransport::Connection;
use wtransport::{RecvStream, SendStream};

use super::client_frames::ClientFrame;
use crate::motion::{MotionIntent, MotionStatus};
use crate::state::SharedState;
use crate::types::{
    LogEntry, MotorFeedback, SafetyEvent, SystemSnapshot, TestProgress, WtEnvelope, WtKind,
    WtPayload, WtSubscribe, WtSubscribeFilters, WtTransport,
};

/// Match REST `/api/motors/:role/motion/jog`: unbounded jog has no automatic
/// reversal, so WT client streams must not bypass the same dead-man cap.
const MAX_WT_JOG_VEL_RAD_S: f32 = 0.5;

fn clamp_wt_jog_velocity(vel_rad_s: f32) -> Option<f32> {
    vel_rad_s
        .is_finite()
        .then(|| vel_rad_s.clamp(-MAX_WT_JOG_VEL_RAD_S, MAX_WT_JOG_VEL_RAD_S))
}

fn ensure_wt_motion_control(
    state: &SharedState,
    role: &str,
    session_id: Option<&str>,
    op: &str,
) -> bool {
    let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
        warn!(role = %role, op, "wt: motion rejected without session_id");
        return false;
    };
    match state.ensure_control(session_id) {
        Ok(()) => true,
        Err(holder) => {
            warn!(
                role = %role,
                op,
                holder = %holder,
                "wt: motion rejected by control lock"
            );
            false
        }
    }
}

/// Per-session router: owns subscriptions, sequence counters, the lazily-
/// opened reliable stream handle, and a CBOR scratch buffer reused across
/// frames.
pub struct SessionRouter {
    /// Filter state. Mutable: per-stream tasks call `apply_subscribe` on
    /// inbound `ClientFrame::Subscribe` and the broadcast fan-out below
    /// reads it via the `Arc<RwLock<_>>` clone. Default = "everything".
    filter: Arc<RwLock<SubscriptionFilter>>,

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
pub struct SubscriptionFilter {
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

    fn allows_run(&self, run_id: &str) -> bool {
        let runs = &self.sub.run_ids;
        runs.is_empty() || runs.iter().any(|r| r == run_id)
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
            filter: Arc::new(RwLock::new(SubscriptionFilter::default())),
            seq: HashMap::new(),
            reliable_stream: None,
            scratch: Vec::with_capacity(256),
        }
    }

    /// Clone the filter handle so per-stream tasks can update it from
    /// outside the broadcast fan-out loop.
    pub fn filter_handle(&self) -> Arc<RwLock<SubscriptionFilter>> {
        self.filter.clone()
    }

    /// Replace the active filter with one parsed from a client `WtSubscribe`
    /// message. Empty `kinds` ≡ "all" (matches the default behavior).
    pub fn apply_subscribe(filter: &Arc<RwLock<SubscriptionFilter>>, sub: WtSubscribe) {
        let new = SubscriptionFilter {
            kinds: if sub.kinds.is_empty() {
                None
            } else {
                Some(sub.kinds)
            },
            sub: sub.filters,
        };
        debug!("wt: applied subscription filter {:?}", new);
        *filter.write().expect("filter poisoned") = new;
    }

    /// Should this kind+payload pair be forwarded to the peer?
    /// Cheap; called per frame on the hot path.
    pub fn allows_motor_feedback(&self, fb: &MotorFeedback) -> bool {
        let f = self.filter.read().expect("filter poisoned");
        f.allows_kind(WtKind::MotorFeedback) && f.allows_motor(&fb.role)
    }

    pub fn allows_system_snapshot(&self) -> bool {
        let f = self.filter.read().expect("filter poisoned");
        f.allows_kind(WtKind::SystemSnapshot)
    }

    pub fn allows_test_progress(&self, p: &TestProgress) -> bool {
        let f = self.filter.read().expect("filter poisoned");
        f.allows_kind(WtKind::TestProgress) && f.allows_motor(&p.role) && f.allows_run(&p.run_id)
    }

    pub fn allows_safety_event(&self) -> bool {
        let f = self.filter.read().expect("filter poisoned");
        f.allows_kind(WtKind::SafetyEvent)
    }

    /// `motion_status` reuses the `motor_roles` filter so a per-detail-page
    /// subscriber can narrow to one role without inventing a new filter
    /// dimension. Empty roles ≡ all roles.
    pub fn allows_motion_status(&self, ms: &MotionStatus) -> bool {
        let f = self.filter.read().expect("filter poisoned");
        f.allows_kind(WtKind::MotionStatus) && f.allows_motor(&ms.role)
    }

    pub fn allows_log_event(&self) -> bool {
        let f = self.filter.read().expect("filter poisoned");
        f.allows_kind(WtKind::LogEvent)
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
                trace!(
                    "wt: dgram kind={} seq={} bytes={}",
                    kind.as_str(),
                    seq,
                    self.scratch.len()
                );
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
    let filter_handle = router.filter_handle();
    let mut feedback_rx = state.feedback_tx.subscribe();
    let mut system_rx = state.system_tx.subscribe();
    let mut tests_rx = state.test_progress_tx.subscribe();
    let mut safety_rx = state.safety_event_tx.subscribe();
    let mut motion_rx = state.motion_status_tx.subscribe();
    let mut log_rx = state.log_event_tx.subscribe();

    // Spawn an accept loop for inbound bidi streams. Each accepted
    // stream becomes its own task running [`run_client_stream`]; that
    // task owns the lifetime of any motion it spawned (so a tab close
    // ≡ stream EOF ≡ MotionStopReason::ClientGone).
    //
    // Wrap the connection in `Arc` so the accept task and the broadcast
    // fan-out below can both reach it (the latter via `&*connection`).
    let connection = Arc::new(connection);
    let accept_task = {
        let connection = connection.clone();
        let state = state.clone();
        let filter = filter_handle.clone();
        tokio::spawn(async move {
            loop {
                let stream = match connection.accept_bi().await {
                    Ok(s) => s,
                    Err(e) => {
                        debug!("wt: accept_bi loop ending: {e:#}");
                        return;
                    }
                };
                let state = state.clone();
                let filter = filter.clone();
                tokio::spawn(async move {
                    let (send, recv) = stream;
                    if let Err(e) = run_client_stream(send, recv, state, filter).await {
                        debug!("wt: client stream ended: {e:#}");
                    }
                });
            }
        })
    };

    let conn_ref = connection.as_ref();
    let res: Result<()> = loop {
        tokio::select! {
            // (b1) Motor feedback fan-out.
            res = feedback_rx.recv() => match res {
                Ok(fb) => {
                    if router.allows_motor_feedback(&fb) {
                        if let Err(e) = router.send::<MotorFeedback>(conn_ref, WtKind::MotorFeedback, fb).await {
                            debug!("wt: motor_feedback send failed; closing session: {e:#}");
                            break Ok(());
                        }
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    debug!("wt: feedback receiver lagged {n}");
                }
                Err(RecvError::Closed) => break Ok(()),
            },

            // (b2) System snapshot fan-out.
            res = system_rx.recv() => match res {
                Ok(snap) => {
                    if router.allows_system_snapshot() {
                        if let Err(e) = router.send::<SystemSnapshot>(conn_ref, WtKind::SystemSnapshot, snap).await {
                            debug!("wt: system_snapshot send failed; closing session: {e:#}");
                            break Ok(());
                        }
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    debug!("wt: system receiver lagged {n}");
                }
                Err(RecvError::Closed) => break Ok(()),
            },

            // (b3) Bench-test progress fan-out (reliable).
            res = tests_rx.recv() => match res {
                Ok(p) => {
                    if router.allows_test_progress(&p) {
                        if let Err(e) = router.send::<TestProgress>(conn_ref, WtKind::TestProgress, p).await {
                            debug!("wt: test_progress send failed; closing session: {e:#}");
                            break Ok(());
                        }
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    debug!("wt: tests receiver lagged {n}");
                }
                Err(RecvError::Closed) => break Ok(()),
            },

            // (b4) Safety-event fan-out (reliable).
            res = safety_rx.recv() => match res {
                Ok(ev) => {
                    if router.allows_safety_event() {
                        if let Err(e) = router.send::<SafetyEvent>(conn_ref, WtKind::SafetyEvent, ev).await {
                            debug!("wt: safety_event send failed; closing session: {e:#}");
                            break Ok(());
                        }
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    debug!("wt: safety receiver lagged {n}");
                }
                Err(RecvError::Closed) => break Ok(()),
            },

            // (b5) Live motion-status fan-out (datagram). Filtered by
            // motor role via the existing `motor_roles` sub-filter so a
            // detail page can narrow to one motor without a new filter
            // dimension.
            res = motion_rx.recv() => match res {
                Ok(ms) => {
                    if router.allows_motion_status(&ms) {
                        if let Err(e) = router.send::<MotionStatus>(conn_ref, WtKind::MotionStatus, ms).await {
                            debug!("wt: motion_status send failed; closing session: {e:#}");
                            break Ok(());
                        }
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    debug!("wt: motion receiver lagged {n}");
                }
                Err(RecvError::Closed) => break Ok(()),
            },

            // (b6) Live log fan-out (reliable). Captures both the
            // tracing-layer events and the audit-log fanout — see
            // `log_layer` and `audit::AuditLog::write`.
            res = log_rx.recv() => match res {
                Ok(le) => {
                    if router.allows_log_event() {
                        if let Err(e) = router.send::<LogEntry>(conn_ref, WtKind::LogEvent, le).await {
                            debug!("wt: log_event send failed; closing session: {e:#}");
                            break Ok(());
                        }
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    debug!("wt: log receiver lagged {n}");
                }
                Err(RecvError::Closed) => break Ok(()),
            },
        }
    };

    accept_task.abort();
    res
}

/// Drive one inbound bidi stream to completion.
///
/// Each accepted bidi stream is its own task. The task reads
/// length-prefixed CBOR `ClientFrame`s in a loop and dispatches each
/// one. The task tracks the (one) role it has spawned a motion for, so
/// that an EOF without a preceding `MotionStop` can issue a clean
/// `MotionStopReason::ClientGone` stop on the operator's behalf.
///
/// The send half of the bidi stream is currently unused (no per-frame
/// ack); it stays open so future replies (e.g. "intent rejected, here's
/// why") can be added without re-litigating the protocol.
async fn run_client_stream(
    _send: SendStream,
    recv: RecvStream,
    state: SharedState,
    filter: Arc<RwLock<SubscriptionFilter>>,
) -> Result<()> {
    let mut owned_role: Option<String> = None;
    let result = read_client_frames(recv, &state, &filter, &mut owned_role).await;

    if let Some(role) = owned_role {
        // EOF (or error) without a `MotionStop` first → treat as client
        // gone. `stop()` is idempotent so a stop frame that *did* arrive
        // before we noticed the stream closing isn't double-counted.
        if state.motion.stop(&role).await {
            debug!(role = %role, "wt: bidi stream closed; stopped owned motion");
        }
    }

    result
}

/// Per-frame loop. Returns `Ok(())` on clean EOF, `Err` on framing /
/// decode error or QUIC-level I/O failure.
async fn read_client_frames(
    mut recv: RecvStream,
    state: &SharedState,
    filter: &Arc<RwLock<SubscriptionFilter>>,
    owned_role: &mut Option<String>,
) -> Result<()> {
    /// Cap on a single frame body. ClientFrames are small (largest is a
    /// `Subscribe` carrying a few kinds + a few roles); 8 KiB matches the
    /// previous one-shot subscribe cap.
    const MAX_FRAME_BYTES: usize = 8 * 1024;
    /// Cap on total bytes per stream lifetime. A jog session that runs
    /// for 8 hours at 5 Hz × ~64 bytes/frame is ~6 MiB; budget 64 MiB so
    /// a stuck-finger jog can run for a day without tripping the cap,
    /// but a malicious client can't stream forever.
    const MAX_STREAM_BYTES: usize = 64 * 1024 * 1024;

    let mut total_read: usize = 0;
    let mut buffered: Vec<u8> = Vec::with_capacity(256);
    let mut chunk = [0u8; 1024];

    loop {
        match recv.read(&mut chunk).await {
            Ok(Some(n)) if n > 0 => {
                total_read += n;
                if total_read > MAX_STREAM_BYTES {
                    return Err(anyhow::anyhow!(
                        "client stream exceeded {MAX_STREAM_BYTES} bytes"
                    ));
                }
                buffered.extend_from_slice(&chunk[..n]);
            }
            Ok(_) => break, // None or 0 -> stream finished
            Err(e) => return Err(anyhow::anyhow!("client stream read: {e}")),
        }

        // Drain as many complete length-prefixed frames as the buffer
        // currently holds.
        while buffered.len() >= 4 {
            let len =
                u32::from_be_bytes([buffered[0], buffered[1], buffered[2], buffered[3]]) as usize;
            if len == 0 {
                return Err(anyhow::anyhow!("zero-length client frame"));
            }
            if len > MAX_FRAME_BYTES {
                return Err(anyhow::anyhow!(
                    "client frame {len} bytes > cap {MAX_FRAME_BYTES}"
                ));
            }
            let total = 4 + len;
            if buffered.len() < total {
                break;
            }

            let body = &buffered[4..total];
            let frame: ClientFrame = ciborium::de::from_reader(body)
                .map_err(|e| anyhow::anyhow!("ClientFrame cbor decode: {e}"))?;
            dispatch_client_frame(frame, state, filter, owned_role).await;
            buffered.drain(..total);
        }
    }

    // Best-effort: a stream that closes mid-frame is a protocol error,
    // but a stream that closes cleanly with leftover bytes (less than 4
    // header bytes) is just the operator-tab-closing case. Treat the
    // partial-header tail as a clean EOF.
    if !buffered.is_empty() && buffered.len() < 4 {
        debug!(
            "wt: client stream closed with {} stray byte(s)",
            buffered.len()
        );
    } else if !buffered.is_empty() {
        return Err(anyhow::anyhow!(
            "client stream closed mid-frame with {} buffered bytes",
            buffered.len()
        ));
    }
    Ok(())
}

/// Dispatch one decoded [`ClientFrame`]. The router's filter handle is
/// updated in place for `Subscribe`; motion frames go through
/// `MotionRegistry`.
async fn dispatch_client_frame(
    frame: ClientFrame,
    state: &SharedState,
    filter: &Arc<RwLock<SubscriptionFilter>>,
    owned_role: &mut Option<String>,
) {
    match frame {
        ClientFrame::Subscribe(sub) => {
            SessionRouter::apply_subscribe(filter, sub);
        }
        ClientFrame::MotionJog {
            role,
            vel_rad_s,
            session_id,
        } => {
            if !ensure_wt_motion_control(state, &role, session_id.as_deref(), "motion_jog") {
                return;
            }
            let Some(vel_rad_s) = clamp_wt_jog_velocity(vel_rad_s) else {
                warn!(role = %role, "wt: motion_jog rejected non-finite velocity");
                return;
            };
            let intent = MotionIntent::Jog { vel_rad_s };
            // Hot path: existing jog → just update intent (which also
            // refreshes the heartbeat in the controller).
            let already_jogging = state
                .motion
                .current(&role)
                .map(|s| s.kind == "jog")
                .unwrap_or(false);
            if already_jogging {
                state.motion.update_intent(&role, intent);
                *owned_role = Some(role);
            } else {
                match state.motion.start(state, &role, intent).await {
                    Ok(_run_id) => {
                        *owned_role = Some(role);
                    }
                    Err(e) => {
                        warn!(role = %role, error = e.code(), "wt: motion_jog rejected");
                    }
                }
            }
        }
        ClientFrame::MotionHeartbeat { role, session_id } => {
            if !ensure_wt_motion_control(state, &role, session_id.as_deref(), "motion_heartbeat") {
                return;
            }
            // No-op if the role isn't actively jogging — the controller
            // would have already exited on heartbeat lapse, and we don't
            // want a stale heartbeat to silently re-arm something the
            // operator stopped.
            if !state.motion.heartbeat_jog(&role) {
                trace!(role = %role, "wt: heartbeat for non-jog (or idle) role");
            } else {
                *owned_role = Some(role);
            }
        }
        ClientFrame::MotionStop { role, session_id } => {
            if !ensure_wt_motion_control(state, &role, session_id.as_deref(), "motion_stop") {
                return;
            }
            let stopped = state.motion.stop(&role).await;
            if stopped {
                debug!(role = %role, "wt: client requested motion stop");
            }
            // Whether we stopped or not, the operator's intent is "this
            // stream no longer drives this role" — clear the ownership
            // so the EOF path doesn't re-stop the *next* operator's run.
            if owned_role.as_deref() == Some(role.as_str()) {
                *owned_role = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wt_jog_velocity_clamps_to_rest_deadman_cap() {
        assert_eq!(clamp_wt_jog_velocity(2.0), Some(MAX_WT_JOG_VEL_RAD_S));
        assert_eq!(clamp_wt_jog_velocity(-2.0), Some(-MAX_WT_JOG_VEL_RAD_S));
        assert_eq!(clamp_wt_jog_velocity(0.25), Some(0.25));
    }

    #[test]
    fn wt_jog_velocity_rejects_non_finite() {
        assert_eq!(clamp_wt_jog_velocity(f32::NAN), None);
        assert_eq!(clamp_wt_jog_velocity(f32::INFINITY), None);
    }
}
