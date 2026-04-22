//! Hardware discovery: unassigned CAN IDs and active scan.
//!
//! `GET /api/hardware/unassigned` lists `(bus, can_id)` in `state.seen_can_ids`
//! that are not in inventory. Passive traffic and `POST /api/hardware/scan`
//! both populate that map (with `source` and optional probe metadata).
//! Stale entries (older than `AppState::SEEN_CAN_ID_TTL_MS`) are pruned on
//! every read so a long-unplugged device doesn't linger in the operator UI.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::can;
use crate::discovery::{DiscoveredDevice, ScanAttempt, ScanDiagnostics};
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
    // Lazy TTL eviction: cheap (one pass over the map under a write
    // lock) and avoids a separate background pruner thread for the
    // small number of entries we accumulate.
    let _pruned = state.prune_stale_seen_can_ids();
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

/// Per-id type-17 timeout, in ms. The previous default of 50 ms was too
/// tight for a real bus where the worker has to share send/recv with the
/// per-bus telemetry stream; raising the floor cuts false-negative scans
/// dramatically. The cap is bumped from 500 ms → 2000 ms so the operator
/// can run a deliberately patient scan from the SPA when chasing a flaky
/// device.
const SCAN_DEFAULT_TIMEOUT_MS: u64 = 150;
const SCAN_MIN_TIMEOUT_MS: u64 = 20;
const SCAN_MAX_TIMEOUT_MS: u64 = 2_000;

#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub ok: bool,
    pub discovered: Vec<DiscoveredDevice>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ScanAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "is_default_diag")]
    pub diagnostics: ScanDiagnostics,
}

fn is_default_diag(d: &ScanDiagnostics) -> bool {
    d.buses_scanned == 0
        && d.broadcast_responses == 0
        && d.targeted_probes_sent == 0
        && d.targeted_probes_succeeded == 0
        && d.targeted_probes_timed_out == 0
        && d.elapsed_ms == 0
}

pub async fn scan(
    State(state): State<SharedState>,
    Json(body): Json<ScanBody>,
) -> Json<ScanResponse> {
    let id_min = body.id_min.unwrap_or(1);
    let id_max = body.id_max.unwrap_or(0x7F);
    let timeout_ms = body
        .timeout_ms
        .unwrap_or(SCAN_DEFAULT_TIMEOUT_MS)
        .clamp(SCAN_MIN_TIMEOUT_MS, SCAN_MAX_TIMEOUT_MS);
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
            diagnostics: r.diagnostics,
        }),
        Ok(Err(e)) => Json(ScanResponse {
            ok: false,
            discovered: vec![],
            attempts: vec![],
            message: Some(e.to_string()),
            diagnostics: ScanDiagnostics::default(),
        }),
        Err(e) => Json(ScanResponse {
            ok: false,
            discovered: vec![],
            attempts: vec![],
            message: Some(format!("scan task failed: {e}")),
            diagnostics: ScanDiagnostics::default(),
        }),
    }
}
