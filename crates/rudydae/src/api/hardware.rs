//! Hardware discovery: unassigned CAN IDs and active scan.
//!
//! `GET /api/hardware/unassigned` lists `(bus, can_id)` in `state.seen_can_ids`
//! that are not in inventory. Passive traffic and `POST /api/hardware/scan`
//! both populate that map (with `source` and optional probe metadata).

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::can;
use crate::discovery::{DiscoveredDevice, ScanAttempt};
use crate::state::SharedState;

/// A CAN ID seen on the bus (or reported by a scan) that is not in inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnassignedDevice {
    pub bus: String,
    pub can_id: u8,
    /// `passive` | `active_scan` | `both`
    pub source: String,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identification_payload: Option<serde_json::Value>,
}

pub async fn list_unassigned(State(state): State<SharedState>) -> Json<Vec<UnassignedDevice>> {
    let seen = state
        .seen_can_ids
        .read()
        .expect("seen_can_ids poisoned")
        .clone();
    let inv = state.inventory.read().expect("inventory poisoned");

    let mut out: Vec<UnassignedDevice> = seen
        .into_iter()
        .filter(|((bus, can_id), _)| inv.by_can_id(bus, *can_id).is_none())
        .map(|((bus, can_id), info)| UnassignedDevice {
            bus,
            can_id,
            source: info.source,
            first_seen_ms: info.first_seen_ms,
            last_seen_ms: info.last_seen_ms,
            family_hint: info.family_hint,
            identification_payload: info.identification_payload,
        })
        .collect();

    out.sort_by(|a, b| a.bus.cmp(&b.bus).then(a.can_id.cmp(&b.can_id)));
    Json(out)
}

#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)] // Accepted for forward compatibility; scan implementation pending.
pub struct ScanBody {
    #[serde(default)]
    pub bus: Option<String>,
    #[serde(default)]
    pub id_min: Option<u8>,
    #[serde(default)]
    pub id_max: Option<u8>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub ok: bool,
    pub discovered: Vec<DiscoveredDevice>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ScanAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub async fn scan(State(state): State<SharedState>, Json(body): Json<ScanBody>) -> Json<ScanResponse> {
    let id_min = body.id_min.unwrap_or(1);
    let id_max = body.id_max.unwrap_or(0x7F);
    let timeout_ms = body.timeout_ms.unwrap_or(50).clamp(10, 500);
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let bus = body.bus.clone();

    let report = tokio::task::spawn_blocking(move || {
        can::hardware_active_scan(&state, bus.as_deref(), id_min, id_max, timeout)
    })
    .await;

    match report {
        Ok(Ok(r)) => Json(ScanResponse {
            ok: true,
            discovered: r.discovered,
            attempts: r.attempts,
            message: r.message,
        }),
        Ok(Err(e)) => Json(ScanResponse {
            ok: false,
            discovered: vec![],
            attempts: vec![],
            message: Some(e.to_string()),
        }),
        Err(e) => Json(ScanResponse {
            ok: false,
            discovered: vec![],
            attempts: vec![],
            message: Some(format!("scan task failed: {e}")),
        }),
    }
}
