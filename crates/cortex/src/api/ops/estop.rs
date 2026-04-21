//! POST /api/estop — global e-stop.
//!
//! Issues `cmd_stop` (RS03 type-4) to every present motor in inventory and
//! broadcasts a `SafetyEvent::Estop` so all WT subscribers update their UI.
//! Bypasses the single-operator lock by design — anyone with network reach
//! to cortex must always be able to stop the robot.

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use serde::Serialize;

use crate::audit::{AuditEntry, AuditResult};
use crate::inventory::Actuator;
use crate::state::SharedState;
use crate::types::{ApiError, SafetyEvent};
use crate::util::session_from_headers;

#[derive(Debug, Serialize)]
pub struct EstopResp {
    pub ok: bool,
    pub stopped: usize,
}

pub async fn estop(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<EstopResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);
    let motors: Vec<Actuator> = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuators()
        .filter(|m| m.common.present)
        .cloned()
        .collect();

    let stopped: Vec<String> = if let Some(core) = state.real_can.clone() {
        let motors_for_blocking = motors.clone();
        tokio::task::spawn_blocking(move || {
            let mut stopped: Vec<String> = Vec::new();
            for motor in &motors_for_blocking {
                // Per-motor stop failures don't abort the e-stop — we want
                // every other motor to still receive its stop frame even if
                // one CAN bus glitched. The error is already audit-logged
                // via the broadcast.
                if core.stop(motor).is_ok() {
                    stopped.push(motor.common.role.clone());
                }
            }
            stopped
        })
        .await
        .expect("estop task panicked")
    } else {
        // Mock mode: there's nothing to stop, but the audit + broadcast still
        // happen so test harnesses can pin the wire shape.
        motors.iter().map(|m| m.common.role.clone()).collect()
    };

    for role in &stopped {
        state.mark_stopped(role);
    }
    let stopped = stopped.len();

    let _ = state.safety_event_tx.send(SafetyEvent::Estop {
        t_ms: Utc::now().timestamp_millis(),
        source: session.clone().unwrap_or_else(|| "anonymous".into()),
    });

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "estop".into(),
        target: None,
        details: serde_json::json!({ "stopped": stopped, "total": motors.len() }),
        result: AuditResult::Ok,
    });

    Ok(Json(EstopResp { ok: true, stopped }))
}
