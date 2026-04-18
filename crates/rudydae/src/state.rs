//! Shared application state injected into every axum handler.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::broadcast;

use crate::audit::AuditLog;
use crate::boot_state::BootState;
use crate::can::RealCanHandle;
use crate::config::Config;
use crate::inventory::Inventory;
use crate::reminders::ReminderStore;
use crate::spec::ActuatorSpec;
use crate::system::SystemPoller;
use crate::types::{MotorFeedback, ParamSnapshot, SafetyEvent, SystemSnapshot, TestProgress};

/// Identity record for whichever WebTransport session currently owns the
/// single-operator lock. Exposed so the SPA can render a "held by ..." pill
/// and a "take over" button.
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

    /// Per-motor mutex guarding the auto-recovery routine (Layer 6). The
    /// routine acquires this for the entire duration so two telemetry ticks
    /// can't both spawn recovery for the same motor, and so manual commands
    /// can detect "recovery in progress" by trying to lock with `try_lock`.
    /// Sequential-across-motors policy (one recovery at a time globally) is
    /// enforced by `auto_recovery::GLOBAL_RECOVERY_LOCK`.
    pub auto_recovery_attempted:
        Mutex<std::collections::HashSet<String>>,
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
            params: RwLock::new(BTreeMap::new()),
            feedback_tx,
            system_tx,
            test_progress_tx,
            safety_event_tx,
            control_lock: RwLock::new(None),
            system: Mutex::new(SystemPoller::new()),
            reminders,
            boot_state: RwLock::new(HashMap::new()),
            auto_recovery_attempted: Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Returns true if `session_id` currently holds the lock (or no lock is
    /// held). Mutating handlers consult this before issuing CAN frames; a
    /// `None` return means the request should be rejected with 423 Locked.
    pub fn has_control(&self, session_id: &str) -> bool {
        match &*self.control_lock.read().expect("control_lock poisoned") {
            None => true,
            Some(holder) => holder.session_id == session_id,
        }
    }

    /// Acquire (or take-over) the control lock for `session_id`. Returns the
    /// previous holder, if any, so the caller can audit the take-over.
    pub fn acquire_control(&self, session_id: &str) -> Option<ControlLockHolder> {
        let mut guard = self.control_lock.write().expect("control_lock poisoned");
        let prev = guard.clone();
        *guard = Some(ControlLockHolder {
            session_id: session_id.to_string(),
            acquired_at_ms: chrono::Utc::now().timestamp_millis(),
        });
        prev
    }

    /// Release the lock if `session_id` currently holds it. No-op otherwise.
    pub fn release_control(&self, session_id: &str) -> bool {
        let mut guard = self.control_lock.write().expect("control_lock poisoned");
        if guard
            .as_ref()
            .map(|h| h.session_id == session_id)
            .unwrap_or(false)
        {
            *guard = None;
            true
        } else {
            false
        }
    }
}

pub type SharedState = Arc<AppState>;
