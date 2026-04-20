//! Shared application state injected into every axum handler.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

use crate::audit::{AuditEntry, AuditLog, AuditResult};
use crate::boot_state::BootState;
use crate::can::RealCanHandle;
use crate::config::Config;
use crate::inventory::{Inventory, RobstrideModel};
use crate::log_store::LogStore;
use crate::motion::{MotionRegistry, MotionStatus};
use crate::reminders::ReminderStore;
use crate::spec::ActuatorSpec;
use crate::system::SystemPoller;
use crate::types::{
    LogEntry, MotorFeedback, ParamSnapshot, SafetyEvent, SystemSnapshot, TestProgress,
};

/// Erased setter for the reload-able `EnvFilter`. Stored in `AppState`
/// instead of the concrete `tracing_subscriber::reload::Handle` so the
/// state struct doesn't have to name the `Registry`'s exact subscriber
/// type (which is awkward to spell once layers are stacked).
///
/// The `main.rs` setup function builds a closure over the real handle
/// and hands it to `AppState::attach_filter_handle` once the subscriber
/// is up. `PUT /api/logs/level` fires the closure; failure (e.g. internal
/// reload error) is propagated as a `String`.
pub type FilterReloadFn = Arc<dyn Fn(EnvFilter) -> Result<(), String> + Send + Sync>;

/// Identity record for whichever session currently owns the single-operator
/// lock. The lock is auto-acquired by the first mutating request from a fresh
/// session, so this is mostly an internal bookkeeping struct used to detect
/// "a different tab is already driving" and refuse the second mutator with
/// 423 Locked. There is no operator-facing UI for it.
#[derive(Debug, Clone)]
pub struct ControlLockHolder {
    pub session_id: String,
    pub acquired_at_ms: i64,
}

/// Timestamps for a `(can_bus, can_id)` observed on the wire (passive listen
/// and, later, active scan). Used to populate `GET /api/hardware/unassigned`.
#[derive(Debug, Clone)]
pub struct SeenInfo {
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    /// `passive` | `active_scan` | `both`
    pub source: String,
}

pub struct AppState {
    pub cfg: Config,
    /// Per–RobStride-model hardware spec (`robstride_rs0X.yaml`).
    pub specs: HashMap<RobstrideModel, Arc<ActuatorSpec>>,
    /// Live inventory. Behind a lock so the per-motor PUT endpoints
    /// (`travel_limits`, `verified`) can swap in the freshly-rewritten
    /// disk copy without restarting the daemon.
    pub inventory: RwLock<Inventory>,
    /// CAN node IDs observed on each bus (type-2 / type-17 passive decode).
    /// Keys not present in [`Self::inventory`] surface as unassigned hardware.
    pub seen_can_ids: RwLock<HashMap<(String, u8), SeenInfo>>,
    pub audit: AuditLog,
    pub real_can: Option<Arc<RealCanHandle>>,

    /// In-memory per-motor latest feedback (role -> feedback).
    pub latest: RwLock<BTreeMap<String, MotorFeedback>>,

    /// Per-role timestamp (ms since unix epoch) of the most recent
    /// type-2 (`MotorFeedback`) frame we received from the bus. Distinct
    /// from `latest[role].t_ms`, which tracks *any* refresh — including
    /// the slow type-17 fallback that keeps `latest` warm while a motor
    /// is idle and not emitting type-2 traffic.
    ///
    /// Internal-only (not on the wire); used by the jog stale-telemetry
    /// refusal so its log message can distinguish "type-2 stream
    /// stuttered mid-sweep" (real bus problem) from "motor was idle, the
    /// type-17 round-robin just took an extra tick" (benign edge case
    /// the threshold is sized for). Without this, every false-positive
    /// refusal looks identical to a real one in the logs.
    pub last_type2_at: RwLock<HashMap<String, i64>>,

    /// In-memory per-motor parameter snapshot (role -> snapshot). Written to
    /// whenever the telemetry loop decodes a type-17 read or a write succeeds.
    pub params: RwLock<BTreeMap<String, ParamSnapshot>>,

    /// Broadcast channels for live fan-out to the WebTransport sessions.
    pub feedback_tx: broadcast::Sender<MotorFeedback>,

    /// Periodic host-metrics broadcast (CPU / mem / temps). One sender; the
    /// WT listener subscribes per-session and forwards each snapshot as a
    /// `WtFrame::SystemSnapshot` datagram. Capacity is small because the
    /// producer cadence is ~1-2 s — even a stalled subscriber lagging by a
    /// few seconds is acceptable.
    pub system_tx: broadcast::Sender<SystemSnapshot>,

    /// Per-bench-routine progress. One sender; the WT listener subscribes
    /// per-session and forwards each line as a reliable
    /// `WtFrame::TestProgress` stream frame.
    pub test_progress_tx: broadcast::Sender<TestProgress>,

    /// Safety-event broadcast (e-stop, lock changes, travel-band rejections).
    /// Reliable WT stream so the dashboard never misses an e-stop pulse.
    pub safety_event_tx: broadcast::Sender<SafetyEvent>,

    /// Live snapshots from server-side motion controllers (sweep / wave /
    /// jog). One sender; the WT listener subscribes per-session and
    /// forwards each snapshot as a `WtFrame::MotionStatus` datagram. The
    /// SPA's "running: sweep on shoulder_a" badge reads off this stream
    /// instead of polling.
    pub motion_status_tx: broadcast::Sender<MotionStatus>,

    /// Per-motor active-motion registry. Single source of truth for
    /// "which controller (if any) is driving motor X right now." Both
    /// the REST `/motion/*` endpoints and the WT bidi router enter
    /// through here so the per-motor concurrency invariant
    /// (one controller per motor) is enforced in exactly one place.
    pub motion: MotionRegistry,

    /// Control-lock state: which session id (if any) is allowed to issue
    /// mutating commands (enable / jog / param write / save).
    pub control_lock: RwLock<Option<ControlLockHolder>>,

    /// Host-metrics state. Mutex (not RwLock) because computing the snapshot
    /// requires the previous CPU totals to compute the delta -> always &mut.
    pub system: Mutex<SystemPoller>,

    /// Operator reminders, file-backed at `.rudyd/reminders.json`.
    pub reminders: ReminderStore,

    /// Per-power-cycle boot-time gate state for each motor (role -> state).
    /// Populated on first telemetry tick by `boot_state::classify`; consulted
    /// by every motion-producing endpoint to refuse commands until the
    /// operator runs the slow-ramp homer (Layers 0/2/5 of the boot-time
    /// gate). NOT persisted across daemon restarts: a motor that was Homed
    /// yesterday is back to Unknown after a power cycle, by design.
    pub boot_state: RwLock<HashMap<String, BootState>>,

    /// Set of motor roles currently believed to be `enabled` on the bus
    /// (i.e. driving — torque allowed). Inserted on a successful `enable`
    /// CAN frame; cleared on a successful `stop` (per-motor stop, e-stop,
    /// jog watchdog, or slow-ramp / e-stop cleanup). Consulted by `rename` /
    /// `assign` so we refuse role mutations while the motor is live, but
    /// admit them as soon as the operator clicks STOP.
    ///
    /// Best-effort: this tracks what rudydae *commanded*, not what the
    /// motor is actually doing on the wire. If a frame is dropped between
    /// the success return and the actual stop landing on the bus, we'll
    /// briefly think the motor is stopped while it isn't. The downstream
    /// gates that this protects (rename, assign) are non-motion operations,
    /// so the worst case is a slightly racy role-string change — not a
    /// motion-safety hazard.
    pub enabled: RwLock<BTreeSet<String>>,

    /// Per-role idempotency set for the boot orchestrator's auto-home
    /// flow (commissioned-zero plan, Phase C.5). The orchestrator
    /// inserts a role when it begins, removes it on terminal-failure
    /// states the operator might still resolve (specifically: leaves
    /// the role IN the set on `OffsetChanged` / `HomeFailed` so a
    /// later telemetry tick doesn't re-trigger the same flow without
    /// operator action; removes it on `OutOfBand` so a future
    /// `OutOfBand → InBand` transition retriggers).
    pub boot_orchestrator_attempted: Mutex<std::collections::HashSet<String>>,

    /// Persistent log store handle. Set once at startup by
    /// `attach_log_store`; tests that don't need the Logs API leave it
    /// empty and the read endpoints return 503 in that case.
    pub log_store: OnceLock<LogStore>,

    /// Live broadcast for captured tracing + audit events. The WT router
    /// subscribes per session and the per-session task sends each frame
    /// as a reliable `WtFrame::LogEvent`. Capacity sized for ~5 s of
    /// worst-case bursts at 1 kHz before the slowest subscriber starts
    /// seeing `Lagged` errors (which the router already handles).
    pub log_event_tx: broadcast::Sender<LogEntry>,

    /// Runtime mutator for the global tracing `EnvFilter`. Wired by
    /// `main.rs` after the subscriber is constructed; tests leave it
    /// `None` and `PUT /api/logs/level` returns 503 in that case.
    pub filter_reload: OnceLock<FilterReloadFn>,
}

impl AppState {
    pub fn new(
        cfg: Config,
        specs: HashMap<RobstrideModel, Arc<ActuatorSpec>>,
        inventory: Inventory,
        audit: AuditLog,
        real_can: Option<Arc<RealCanHandle>>,
        reminders: ReminderStore,
    ) -> Self {
        let (log_event_tx, _) = broadcast::channel::<LogEntry>(2048);
        Self::new_with_log_tx(
            cfg,
            specs,
            inventory,
            audit,
            real_can,
            reminders,
            log_event_tx,
        )
    }

    /// Same as `new` but with a caller-supplied log event broadcast.
    /// `main.rs` uses this to share the broadcast with the
    /// `LogCaptureLayer` it builds before AppState exists; tests use the
    /// short form.
    pub fn new_with_log_tx(
        cfg: Config,
        specs: HashMap<RobstrideModel, Arc<ActuatorSpec>>,
        inventory: Inventory,
        audit: AuditLog,
        real_can: Option<Arc<RealCanHandle>>,
        reminders: ReminderStore,
        log_event_tx: broadcast::Sender<LogEntry>,
    ) -> Self {
        let (feedback_tx, _) = broadcast::channel::<MotorFeedback>(512);
        let (system_tx, _) = broadcast::channel::<SystemSnapshot>(8);
        let (test_progress_tx, _) = broadcast::channel::<TestProgress>(256);
        let (safety_event_tx, _) = broadcast::channel::<SafetyEvent>(64);
        let (motion_status_tx, _) = broadcast::channel::<MotionStatus>(256);
        Self {
            cfg,
            specs,
            inventory: RwLock::new(inventory),
            seen_can_ids: RwLock::new(HashMap::new()),
            audit,
            real_can,
            latest: RwLock::new(BTreeMap::new()),
            last_type2_at: RwLock::new(HashMap::new()),
            params: RwLock::new(BTreeMap::new()),
            feedback_tx,
            system_tx,
            test_progress_tx,
            safety_event_tx,
            motion_status_tx,
            motion: MotionRegistry::new(),
            control_lock: RwLock::new(None),
            system: Mutex::new(SystemPoller::new()),
            reminders,
            boot_state: RwLock::new(HashMap::new()),
            enabled: RwLock::new(BTreeSet::new()),
            boot_orchestrator_attempted: Mutex::new(std::collections::HashSet::new()),
            log_store: OnceLock::new(),
            log_event_tx,
            filter_reload: OnceLock::new(),
        }
    }

    /// Resolve the YAML spec for a RobStride model. Panics if startup loading missed a file.
    pub fn spec_for(&self, model: RobstrideModel) -> Arc<ActuatorSpec> {
        self.specs.get(&model).cloned().unwrap_or_else(|| {
            panic!(
                "no ActuatorSpec loaded for {}; add config/actuators/robstride_{}.yaml",
                model.as_spec_label(),
                model.robstride_yaml_suffix()
            )
        })
    }

    /// Wire the persistent log store + fan out the audit log into both it
    /// and the live broadcast. Idempotent; called once from `main.rs`.
    /// Tests skip this and the read endpoints return 503 in that case.
    pub fn attach_log_store(&self, store: LogStore) {
        self.audit.attach_fanout(crate::audit::AuditFanout {
            store: store.clone(),
            live_tx: self.log_event_tx.clone(),
        });
        let _ = self.log_store.set(store);
    }

    /// Wire the runtime `EnvFilter` reload closure. Called once from
    /// `main.rs` after the subscriber is built. Subsequent calls are
    /// no-ops (OnceLock).
    pub fn attach_filter_reload(&self, f: FilterReloadFn) {
        let _ = self.filter_reload.set(f);
    }

    /// Mark a motor as currently enabled on the bus. Idempotent.
    pub fn mark_enabled(&self, role: &str) {
        self.enabled
            .write()
            .expect("enabled poisoned")
            .insert(role.to_string());
    }

    /// Clear a motor's enabled bit. Idempotent. Called from every code path
    /// that successfully sends a `stop` frame (per-motor stop, e-stop, jog
    /// watchdog timeout, slow-ramp cleanup).
    pub fn mark_stopped(&self, role: &str) {
        self.enabled.write().expect("enabled poisoned").remove(role);
    }

    /// Cheap predicate for the `rename` / `assign` gates.
    pub fn is_enabled(&self, role: &str) -> bool {
        self.enabled
            .read()
            .expect("enabled poisoned")
            .contains(role)
    }

    /// Record a node ID seen on `iface` from passive bus traffic (type-2 or
    /// type-17 reply layout). Idempotent per key; refreshes `last_seen_ms`.
    pub fn record_passive_seen(&self, iface: &str, can_id: u8) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut map = self.seen_can_ids.write().expect("seen_can_ids poisoned");
        let key = (iface.to_string(), can_id);
        match map.entry(key) {
            Entry::Occupied(mut e) => {
                let info = e.get_mut();
                info.last_seen_ms = now_ms;
                if info.source == "active_scan" {
                    info.source = "both".into();
                }
            }
            Entry::Vacant(e) => {
                e.insert(SeenInfo {
                    first_seen_ms: now_ms,
                    last_seen_ms: now_ms,
                    source: "passive".into(),
                });
            }
        }
    }

    /// Cooperative single-operator gate. Called by every mutating handler
    /// before it touches the bus. Three outcomes:
    ///
    /// - **Lock free** → claim it for `session_id`, audit a
    ///   `control_lock_auto_acquire` entry, broadcast `LockChanged`, return
    ///   `Ok(())`. This is the common path: open a fresh tab, click anything,
    ///   that tab now owns the bus.
    /// - **Lock already held by `session_id`** → `Ok(())` cheap path.
    /// - **Lock held by a *different* session** → `Err(holder_session_id)`.
    ///   The caller turns this into 423 Locked. This is the only failure
    ///   mode this gate exists for: prevent a stale/forgotten second tab
    ///   from racing the active operator's commands on the CAN bus.
    ///
    /// `session_id` may be empty when the request omitted `X-Rudy-Session`;
    /// in that case the gate refuses to claim the lock (an unidentified
    /// caller can't be a "holder") but still permits the request when no
    /// lock is currently held — matching the curl-friendly posture the rest
    /// of the API takes.
    pub fn ensure_control(&self, session_id: &str) -> Result<(), String> {
        let mut guard = self.control_lock.write().expect("control_lock poisoned");
        match &*guard {
            Some(holder) if holder.session_id == session_id => Ok(()),
            Some(holder) => Err(holder.session_id.clone()),
            None => {
                if session_id.is_empty() {
                    return Ok(());
                }
                let now_ms = chrono::Utc::now().timestamp_millis();
                *guard = Some(ControlLockHolder {
                    session_id: session_id.to_string(),
                    acquired_at_ms: now_ms,
                });
                drop(guard);

                self.audit.write(AuditEntry {
                    timestamp: chrono::Utc::now(),
                    session_id: Some(session_id.to_string()),
                    remote: None,
                    action: "control_lock_auto_acquire".into(),
                    target: None,
                    details: serde_json::json!({}),
                    result: AuditResult::Ok,
                });

                let _ = self.safety_event_tx.send(SafetyEvent::LockChanged {
                    t_ms: now_ms,
                    holder: Some(session_id.to_string()),
                });

                Ok(())
            }
        }
    }
}

pub type SharedState = Arc<AppState>;
