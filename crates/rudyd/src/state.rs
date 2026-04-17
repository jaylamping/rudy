//! Shared application state injected into every axum handler.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use tokio::sync::broadcast;

use crate::audit::AuditLog;
use crate::config::Config;
use crate::inventory::Inventory;
use crate::spec::ActuatorSpec;
use crate::types::{MotorFeedback, ParamSnapshot};

pub struct AppState {
    pub cfg: Config,
    pub spec: ActuatorSpec,
    pub inventory: Inventory,
    pub audit: AuditLog,
    pub auth_token: Option<String>,

    /// In-memory per-motor latest feedback (role -> feedback).
    pub latest: RwLock<BTreeMap<String, MotorFeedback>>,

    /// In-memory per-motor parameter snapshot (role -> snapshot). Written to
    /// whenever the telemetry loop decodes a type-17 read or a write succeeds.
    pub params: RwLock<BTreeMap<String, ParamSnapshot>>,

    /// Broadcast channels for live fan-out to the WebTransport sessions.
    pub feedback_tx: broadcast::Sender<MotorFeedback>,

    /// Control-lock state: which session id (if any) is allowed to issue
    /// mutating commands (enable / jog / param write / save).
    pub control_lock: RwLock<Option<String>>,
}

impl AppState {
    pub fn new(
        cfg: Config,
        spec: ActuatorSpec,
        inventory: Inventory,
        audit: AuditLog,
        auth_token: Option<String>,
    ) -> Self {
        let (feedback_tx, _) = broadcast::channel::<MotorFeedback>(512);
        Self {
            cfg,
            spec,
            inventory,
            audit,
            auth_token,
            latest: RwLock::new(BTreeMap::new()),
            params: RwLock::new(BTreeMap::new()),
            feedback_tx,
            control_lock: RwLock::new(None),
        }
    }

    /// Helper used by control handlers to enforce single-operator semantics.
    pub fn has_control(&self, session_id: &str) -> bool {
        match &*self.control_lock.read().expect("control_lock poisoned") {
            None => true,
            Some(holder) => holder == session_id,
        }
    }
}

pub type SharedState = Arc<AppState>;
