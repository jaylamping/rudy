//! POST /api/motors/:role/ping — single-device CAN presence probe.
//!
//! Read-only: does not require the control lock. Reuses the same targeted
//! type-17 path as `POST /api/hardware/scan`.

use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

use crate::api::error::err;
use crate::can;
use crate::can::discovery::DiscoveredDevice;
use crate::state::SharedState;
use crate::types::ApiError;

const PING_TIMEOUT_MS: u64 = 500;
const PING_MAX_MS: u64 = 2_000;
const PING_MIN_MS: u64 = 20;

#[derive(Debug, Serialize)]
pub struct PingResponse {
    pub ok: bool,
    pub bus: String,
    pub can_id: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware_version: Option<String>,
    pub elapsed_ms: u64,
}

fn firmware_from_device(d: &DiscoveredDevice) -> Option<String> {
    d.identification_payload.as_ref().and_then(|p| {
        p.get("firmware_version_snippet")?
            .as_str()
            .map(String::from)
    })
}

pub async fn ping_motor(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<PingResponse>, (StatusCode, Json<ApiError>)> {
    let motor = {
        let inv = state.inventory.read().expect("inventory poisoned");
        inv.actuator_by_role(&role).cloned().ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            )
        })?
    };
    let bus = motor.common.can_bus.clone();
    let can_id = motor.common.can_id;
    let timeout = Duration::from_millis(PING_TIMEOUT_MS.max(PING_MIN_MS).min(PING_MAX_MS));

    let st = state.clone();
    let bus_for_probe = bus.clone();
    let started = Instant::now();
    let report = tokio::task::spawn_blocking(move || {
        can::hardware_probe_one(&st, &bus_for_probe, can_id, timeout)
    })
    .await
    .map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ping_task_failed",
            Some(e.to_string()),
        )
    })?
    .map_err(|e| {
        err(
            StatusCode::BAD_REQUEST,
            "ping_failed",
            Some(format!("{e:#}")),
        )
    })?;

    let (discovered, _attempts) = report;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let (ok, firmware_version) = match &discovered {
        Some(d) => (true, firmware_from_device(d)),
        None => (false, None),
    };
    Ok(Json(PingResponse {
        ok,
        bus,
        can_id,
        firmware_version,
        elapsed_ms,
    }))
}
