//! Hardware discovery: unassigned CAN IDs and active scan.
//!
//! `GET /api/hardware/unassigned` is wired; population depends on passive bus
//! tracking (`seen_can_ids`) and optional scan results — see the polymorphic
//! inventory plan Phase D.

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

/// Stub until `passive-seen-ids-tracker` + scan cache land.
pub async fn list_unassigned(State(_state): State<SharedState>) -> Json<Vec<UnassignedDevice>> {
    Json(vec![])
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
