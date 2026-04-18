//! Shared application state injected into every axum handler.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::broadcast;

use crate::audit::AuditLog;
use crate::can::RealCanHandle;
use crate::config::Config;
use crate::inventory::Inventory;
use crate::reminders::ReminderStore;
use crate::spec::ActuatorSpec;
use crate::system::SystemPoller;
use crate::types::{MotorFeedback, ParamSnapshot};

pub struct AppState {
    pub cfg: Config,
    pub spec: ActuatorSpec,
    pub inventory: Inventory,
    pub audit: AuditLog,
    pub real_can: Option<Arc<RealCanHandle>>,

    /// In-memory per-motor latest feedback (role -> feedback).
    pub latest: RwLock<BTreeMap<String, MotorFeedback>>,

    /// In-memory per-motor parameter snapshot (role -> snapshot). Written to
    /// whenever the telemetry loop decodes a type-17 read or a write succeeds.
    pub params: RwLock<BTreeMap<String, ParamSnapshot>>,

    /// Broadcast channels for live fan-out to the WebTransport sessions.
    pub feedback_tx: broadcast::Sender<MotorFeedback>,

    /// Control-lock state: which session id (if any) is allowed to issue
    /// mutating commands (enable / jog / param write / save).
    #[allow(dead_code)]
    pub control_lock: RwLock<Option<String>>,

    /// Host-metrics state. Mutex (not RwLock) because computing the snapshot
    /// requires the previous CPU totals to compute the delta -> always &mut.
    pub system: Mutex<SystemPoller>,

    /// Operator reminders, file-backed at `.rudyd/reminders.json`.
    pub reminders: ReminderStore,
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
        Self {
            cfg,
            spec,
            inventory,
            audit,
            real_can,
            latest: RwLock::new(BTreeMap::new()),
            params: RwLock::new(BTreeMap::new()),
            feedback_tx,
            control_lock: RwLock::new(None),
            system: Mutex::new(SystemPoller::new()),
            reminders,
        }
    }

    /// Helper used by control handlers to enforce single-operator semantics.
    #[allow(dead_code)]
    pub fn has_control(&self, session_id: &str) -> bool {
        match &*self.control_lock.read().expect("control_lock poisoned") {
            None => true,
            Some(holder) => holder == session_id,
        }
    }
}

pub type SharedState = Arc<AppState>;
