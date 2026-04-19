//! Shared application state injected into every axum handler.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::broadcast;

use crate::audit::{AuditEntry, AuditLog, AuditResult};
use crate::boot_state::BootState;
use crate::can::RealCanHandle;
use crate::config::Config;
use crate::inventory::Inventory;
use crate::reminders::ReminderStore;
use crate::spec::ActuatorSpec;
use crate::system::SystemPoller;
use crate::types::{MotorFeedback, ParamSnapshot, SafetyEvent, SystemSnapshot, TestProgress};

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

pub struct AppState {
    pub cfg: Config,
    pub spec: ActuatorSpec,
    /// Live inventory. Behind a lock so the per-motor PUT endpoints
    /// (`travel_limits`, `verified`) can swap in the freshly-rewritten
    /// disk copy without restarting the daemon.
    pub inventory: RwLock<Inventory>,
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
    /// jog watchdog, or auto-recovery cleanup). Consulted by `rename` /
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

    /// Per-motor mutex guarding the auto-recovery routine (Layer 6). The
    /// routine acquires this for the entire duration so two telemetry ticks
    /// can't both spawn recovery for the same motor, and so manual commands
    /// can detect "recovery in progress" by trying to lock with `try_lock`.
    /// Sequential-across-motors policy (one recovery at a time globally) is
    /// enforced by `auto_recovery::GLOBAL_RECOVERY_LOCK`.
    pub auto_recovery_attempted: Mutex<std::collections::HashSet<String>>,
}

impl AppState {
    pub fn new(
        cfg: Config,
        spec: ActuatorSpec,
        inventory: Inventory,
        audit: AuditLog,
        real_can: Option<Arc<RealCanHandle>>,
        reminders: ReminderStore,
    ) -> Self {
        let (feedback_tx, _) = broadcast::channel::<MotorFeedback>(512);
        let (system_tx, _) = broadcast::channel::<SystemSnapshot>(8);
        let (test_progress_tx, _) = broadcast::channel::<TestProgress>(256);
        let (safety_event_tx, _) = broadcast::channel::<SafetyEvent>(64);
        Self {
            cfg,
            spec,
            inventory: RwLock::new(inventory),
            audit,
            real_can,
            latest: RwLock::new(BTreeMap::new()),
            last_type2_at: RwLock::new(HashMap::new()),
            params: RwLock::new(BTreeMap::new()),
            feedback_tx,
            system_tx,
            test_progress_tx,
            safety_event_tx,
            control_lock: RwLock::new(None),
            system: Mutex::new(SystemPoller::new()),
            reminders,
            boot_state: RwLock::new(HashMap::new()),
            enabled: RwLock::new(BTreeSet::new()),
            auto_recovery_attempted: Mutex::new(std::collections::HashSet::new()),
        }
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
    /// watchdog timeout, auto-recovery cleanup).
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
