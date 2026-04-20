//! Hardware discovery: unassigned CAN IDs and active scan.
//!
//! `GET /api/hardware/unassigned` lists `(bus, can_id)` in `state.seen_can_ids`
//! that are not in inventory (passive traffic). Active scan merges are still
//! future work — see the polymorphic inventory plan Phase D.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

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
            family_hint: None,
            identification_payload: None,
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
    /// Devices discovered this run (empty until probes are implemented).
    pub discovered: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Stub: accepts body for forward compatibility; returns empty `discovered`.
pub async fn scan(State(_state): State<SharedState>, Json(_body): Json<ScanBody>) -> Json<ScanResponse> {
    Json(ScanResponse {
        ok: true,
        discovered: vec![],
        message: Some(
            "active scan not implemented yet — passive CAN tracking and device probes pending"
                .into(),
        ),
    })
}
