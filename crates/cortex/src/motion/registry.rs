//! Per-motor active-motion registry.
//!
//! The registry is the single source of truth for "which motor is being
//! driven by which controller right now." Both the REST handlers in
//! [`crate::api::motion`] and the WebTransport bidi router in
//! [`crate::wt_router`] enter through here so the per-motor concurrency
//! invariant ("one controller per motor at a time") is enforced in
//! exactly one place.
//!
//! Lifecycle:
//!
//! * [`MotionRegistry::start`] takes a fresh [`MotionIntent`], runs the
//!   shared preflight, and either spawns a new controller or replaces
//!   the existing one for the same role (the previous controller is
//!   sent the `Superseded` stop signal and joins on its own exit).
//! * [`MotionRegistry::stop`] flips the cancellation flag for one role
//!   and returns once the controller has issued `cmd_stop`.
//! * [`MotionRegistry::update_intent`] swaps the live intent (used for
//!   slider-while-held velocity updates on a jog).
//! * [`MotionRegistry::heartbeat_jog`] refreshes the dead-man deadline
//!   without rewriting the intent.
//!
//! A motor's slot is freed automatically when its controller's task
//! exits (the registry sweeps stale handles every call). No explicit
//! "release" is needed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use chrono::Utc;
use tokio::sync::{watch, Notify};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::motion::controller::{self, ControllerTask};
use crate::motion::intent::MotionIntent;
use crate::motion::preflight::{PreflightChecks, PreflightFailure};
use crate::state::SharedState;

/// Live handle to one running controller. Owned by the [`MotionRegistry`]
/// and dropped on stop / supersession / task exit.
struct MotionHandle {
    run_id: String,
    role: String,
    intent_tx: watch::Sender<MotionIntent>,
    stop: Arc<Notify>,
    superseded: Arc<AtomicBool>,
    /// Heartbeat refresh signal. Sending on the watch channel side
    /// effects nothing on the controller (it borrows the value, not
    /// recvs) — refreshing the heartbeat goes through `Notify` instead.
    heartbeat: Arc<Notify>,
    join: JoinHandle<()>,
    /// Wallclock at start, ms since unix epoch. Used for the GET
    /// snapshot endpoint and for audit-log correlation.
    started_at_ms: i64,
}

/// Public snapshot of one running motion. Returned by `current()` and
/// `GET /api/motors/:role/motion`.
#[derive(Debug, Clone)]
pub struct MotionSnapshot {
    pub run_id: String,
    pub role: String,
    pub kind: String,
    pub intent: MotionIntent,
    pub started_at_ms: i64,
}

/// Registry. One per `AppState`. All operations are cheap (lock + map
/// op + `notify_one`); the actual work happens in the controller task.
#[derive(Default)]
pub struct MotionRegistry {
    inner: RwLock<HashMap<String, MotionHandle>>,
}

impl MotionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start (or supersede) a motion for `role`. Runs the shared
    /// preflight; on success spawns a controller, returning the
    /// freshly-allocated `run_id`.
    ///
    /// If a previous motion is already running for this role, it is
    /// stopped (`MotionStopReason::Superseded`) and joined before the
    /// new controller is registered. This enforces the per-motor
    /// concurrency invariant from the plan: a fresh sweep request
    /// while a wave is running cleanly transitions to the sweep.
    pub async fn start(
        &self,
        state: &SharedState,
        role: &str,
        intent: MotionIntent,
    ) -> Result<String, PreflightFailure> {
        // Preflight against vel=0 with a one-tick horizon. This catches
        // obvious "you can't move this motor right now" failures
        // (Unknown / OutOfBand / stale telemetry) before we tear down
        // the existing motion.
        let preflight = PreflightChecks {
            state,
            role,
            vel_rad_s: 0.0,
            horizon_ms: 10,
            target_position_rad: None,
        };
        let pf = preflight.run()?;

        // Tear down any prior motion for this role. Best-effort: we
        // wait for the join to ensure no two controllers race the bus.
        if let Some(prev) = self.take(role) {
            prev.superseded.store(true, Ordering::Release);
            prev.stop.notify_one();
            // Best-effort join — bound the wait so a wedged controller
            // can't deadlock the new request. The controller itself
            // always issues cmd_stop before exiting, so even if we time
            // out the bus is safe (the new controller will refresh the
            // velocity setpoint on its first tick anyway).
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), prev.join).await;
        }

        let run_id = Uuid::new_v4().to_string();
        let (intent_tx, intent_rx) = watch::channel(intent.clone());
        let stop = Arc::new(Notify::new());
        let superseded = Arc::new(AtomicBool::new(false));
        let heartbeat = Arc::new(Notify::new());
        let shutdown = Arc::new(Notify::new());

        let task = ControllerTask {
            state: state.clone(),
            motor: pf.motor.clone(),
            run_id: run_id.clone(),
            intent_rx,
            stop: stop.clone(),
            superseded: superseded.clone(),
            shutdown,
        };

        let join = tokio::spawn(controller::run(task));

        let handle = MotionHandle {
            run_id: run_id.clone(),
            role: role.to_string(),
            intent_tx,
            stop,
            superseded,
            heartbeat,
            join,
            started_at_ms: Utc::now().timestamp_millis(),
        };
        self.inner
            .write()
            .expect("motion registry poisoned")
            .insert(role.to_string(), handle);

        Ok(run_id)
    }

    /// Stop the running motion for `role`, if any. Returns `true` if a
    /// motion was running, `false` if the role was already idle.
    pub async fn stop(&self, role: &str) -> bool {
        let Some(handle) = self.take(role) else {
            return false;
        };
        handle.stop.notify_one();
        let _ = tokio::time::timeout(std::time::Duration::from_millis(500), handle.join).await;
        true
    }

    /// Hot-swap the intent for an already-running motion (slider drag
    /// while a jog is held). Returns `false` if no motion is active for
    /// `role`. The controller picks up the new intent on the next
    /// `intent_rx.changed()` wakeup, which fires the same tick.
    pub fn update_intent(&self, role: &str, intent: MotionIntent) -> bool {
        let guard = self.inner.read().expect("motion registry poisoned");
        let Some(handle) = guard.get(role) else {
            return false;
        };
        handle.intent_tx.send_replace(intent);
        true
    }

    /// Refresh the dead-man heartbeat for a jog motion. No-op if `role`
    /// isn't a jog (or isn't running).
    pub fn heartbeat_jog(&self, role: &str) -> bool {
        let guard = self.inner.read().expect("motion registry poisoned");
        let Some(handle) = guard.get(role) else {
            return false;
        };
        // The controller watches `intent_rx.changed()`, which we trigger
        // by re-sending the current intent. Cheap (the watch channel
        // dedupes on identity but we want the wakeup, so use
        // `send_replace` unconditionally). The controller's jog branch
        // refreshes its own deadline on every intent change.
        let cur = handle.intent_tx.borrow().clone();
        if matches!(cur, MotionIntent::Jog { .. }) {
            handle.intent_tx.send_replace(cur);
            true
        } else {
            false
        }
    }

    /// Snapshot of the active motion for `role`, if any. The returned
    /// intent is a clone of the most recently-applied value.
    pub fn current(&self, role: &str) -> Option<MotionSnapshot> {
        let guard = self.inner.read().expect("motion registry poisoned");
        let h = guard.get(role)?;
        let intent = h.intent_tx.borrow().clone();
        Some(MotionSnapshot {
            run_id: h.run_id.clone(),
            role: h.role.clone(),
            kind: intent.kind_str().to_string(),
            intent,
            started_at_ms: h.started_at_ms,
        })
    }

    /// Snapshot of every active motion. Used by the WT bidi router to
    /// replay state into a fresh client.
    pub fn snapshot_all(&self) -> Vec<MotionSnapshot> {
        let guard = self.inner.read().expect("motion registry poisoned");
        guard
            .values()
            .map(|h| {
                let intent = h.intent_tx.borrow().clone();
                MotionSnapshot {
                    run_id: h.run_id.clone(),
                    role: h.role.clone(),
                    kind: intent.kind_str().to_string(),
                    intent,
                    started_at_ms: h.started_at_ms,
                }
            })
            .collect()
    }

    /// Pull a handle out of the map and return it for cleanup. Used
    /// internally by `start`/`stop`.
    fn take(&self, role: &str) -> Option<MotionHandle> {
        let mut guard = self.inner.write().expect("motion registry poisoned");
        // Sweep stale handles whose join handle has finished — their
        // controllers exited on their own (heartbeat lapse, fault, etc.)
        // and the slot should be free.
        let mut stale: Vec<String> = Vec::new();
        for (k, h) in guard.iter() {
            if h.join.is_finished() && k != role {
                stale.push(k.clone());
            }
        }
        for k in stale {
            guard.remove(&k);
        }
        guard.remove(role)
    }

    /// Convenience: silence dead_code on `heartbeat`. Held in the handle
    /// so future code (e.g. a separate "ping the controller" path that
    /// doesn't piggyback on the intent watch) has somewhere to plug in.
    #[allow(dead_code)]
    pub(crate) fn _heartbeat_handle(&self, role: &str) -> Option<Arc<Notify>> {
        let guard = self.inner.read().expect("motion registry poisoned");
        guard.get(role).map(|h| h.heartbeat.clone())
    }
}
