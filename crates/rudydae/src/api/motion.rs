//! POST /api/motors/:role/motion/{sweep,wave,jog,stop}
//! GET  /api/motors/:role/motion
//!
//! REST entry point for the server-side motion controllers. Replaces the
//! per-frame `setInterval(api.jog())` pattern the SPA used to implement
//! sweep / wave with a single "start the loop, then watch the WT
//! `motion_status` stream" round-trip per run.
//!
//! All four POSTs share the same:
//!
//! * control-lock check (mutating endpoints; the lock is per-session),
//! * inventory presence + `present` / `verified` gate,
//! * preflight (delegated to [`crate::motion::preflight`] inside
//!   [`crate::motion::registry::MotionRegistry::start`]),
//! * audit-log shape (`motion_start` + intent serialized to JSON).
//!
//! The handlers are intentionally thin — almost all logic lives in
//! `motion::`. See `crates/rudydae/src/motion/mod.rs` for the convention
//! about why closed-loop motion never lives in the SPA.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use http::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::audit::{AuditEntry, AuditResult};
use crate::motion::intent::default_turnaround_rad;
use crate::motion::{MotionIntent, PreflightFailure};
use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

/// Soft cap aligned with `bench_tool::MAX_TARGET_VEL_RAD_S`. Motion
/// requests above this are clamped before the registry sees them; the
/// controller's per-tick preflight is the second line of defense if the
/// firmware envelope is wider than this.
const MAX_MOTION_VEL_RAD_S: f32 = 0.5;

fn err(status: StatusCode, error: &str, detail: Option<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
            detail,
        }),
    )
}

/// Map a [`PreflightFailure`] to an HTTP error envelope. Mirrors the
/// status codes the existing `/jog` handler returns for the equivalent
/// failure shapes so SPA error-handling code is identical.
fn preflight_to_http(role: &str, e: PreflightFailure) -> (StatusCode, Json<ApiError>) {
    let status = match &e {
        PreflightFailure::UnknownMotor => StatusCode::NOT_FOUND,
        PreflightFailure::Absent => StatusCode::CONFLICT,
        PreflightFailure::NotVerified => StatusCode::FORBIDDEN,
        PreflightFailure::BootNotReady { .. }
        | PreflightFailure::BootOutOfBand { .. }
        | PreflightFailure::AutoRecoveryInProgress
        | PreflightFailure::StaleTelemetry { .. }
        | PreflightFailure::NoTelemetry
        | PreflightFailure::OutOfBand { .. }
        | PreflightFailure::PathViolation { .. }
        | PreflightFailure::StepTooLarge { .. } => StatusCode::CONFLICT,
        PreflightFailure::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    err(status, e.code(), Some(e.detail(role)))
}

fn ensure_lock(
    state: &SharedState,
    headers: &HeaderMap,
) -> Result<Option<String>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(headers);
    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
        ));
    }
    Ok(session)
}

fn clamp_speed(speed: f32) -> f32 {
    if !speed.is_finite() {
        return 0.0;
    }
    speed.clamp(-MAX_MOTION_VEL_RAD_S, MAX_MOTION_VEL_RAD_S)
}

#[derive(Debug, Deserialize)]
pub struct SweepBody {
    pub speed_rad_s: f32,
    /// Optional turnaround inset; defaults to
    /// [`crate::motion::intent::default_turnaround_rad`] when omitted.
    pub turnaround_rad: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct WaveBody {
    pub center_rad: f32,
    pub amplitude_rad: f32,
    pub speed_rad_s: f32,
    pub turnaround_rad: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct JogBody {
    pub vel_rad_s: f32,
}

#[derive(Debug, Serialize)]
pub struct StartResp {
    pub run_id: String,
    pub clamped_speed_rad_s: f32,
}

#[derive(Debug, Serialize)]
pub struct StopResp {
    pub stopped: bool,
}

/// GET /api/motors/:role/motion
///
/// Returns 200 with a snapshot of the running motion or 204 if the role
/// is idle. The SPA uses this both for the initial paint of a detail
/// page (so the badge is correct before the WT stream catches up) and
/// as the "did the stop actually take" fallback when the terminal
/// `MotionStatus { state = stopped }` datagram is dropped.
#[derive(Debug, Serialize)]
pub struct MotionSnapshotDto {
    pub run_id: String,
    pub role: String,
    pub kind: String,
    pub started_at_ms: i64,
    pub intent: MotionIntent,
}

pub async fn get_motion(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<MotionSnapshotDto>, StatusCode> {
    match state.motion.current(&role) {
        Some(snap) => Ok(Json(MotionSnapshotDto {
            run_id: snap.run_id,
            role: snap.role,
            kind: snap.kind,
            started_at_ms: snap.started_at_ms,
            intent: snap.intent,
        })),
        None => Err(StatusCode::NO_CONTENT),
    }
}

/// POST /api/motors/:role/motion/sweep
pub async fn start_sweep(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<SweepBody>,
) -> Result<Json<StartResp>, (StatusCode, Json<ApiError>)> {
    let session = ensure_lock(&state, &headers)?;
    let speed = clamp_speed(body.speed_rad_s).abs();
    let intent = MotionIntent::Sweep {
        speed_rad_s: speed,
        turnaround_rad: body
            .turnaround_rad
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or_else(|| {
                default_turnaround_rad(&MotionIntent::Sweep {
                    speed_rad_s: speed,
                    turnaround_rad: 0.0,
                })
            }),
    };
    start(&state, session, &role, intent, speed).await
}

/// POST /api/motors/:role/motion/wave
pub async fn start_wave(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<WaveBody>,
) -> Result<Json<StartResp>, (StatusCode, Json<ApiError>)> {
    let session = ensure_lock(&state, &headers)?;
    let speed = clamp_speed(body.speed_rad_s).abs();
    if !body.center_rad.is_finite() || !body.amplitude_rad.is_finite() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "bad_request",
            Some("center_rad and amplitude_rad must be finite".into()),
        ));
    }
    let amp = body.amplitude_rad.abs();
    let intent = MotionIntent::Wave {
        center_rad: body.center_rad,
        amplitude_rad: amp,
        speed_rad_s: speed,
        turnaround_rad: body
            .turnaround_rad
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or_else(|| {
                default_turnaround_rad(&MotionIntent::Wave {
                    center_rad: 0.0,
                    amplitude_rad: 0.0,
                    speed_rad_s: 0.0,
                    turnaround_rad: 0.0,
                })
            }),
    };
    start(&state, session, &role, intent, speed).await
}

/// POST /api/motors/:role/motion/jog
///
/// Starts (or refreshes the heartbeat / velocity of) a server-side jog.
/// Subsequent POSTs to the same `role` do NOT spawn a fresh controller —
/// they update the live intent and refresh the dead-man window. This is
/// the REST fallback path; the WT bidi stream is the preferred transport
/// for hold-to-jog because it avoids per-heartbeat HTTP overhead.
pub async fn start_or_update_jog(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<JogBody>,
) -> Result<Json<StartResp>, (StatusCode, Json<ApiError>)> {
    let session = ensure_lock(&state, &headers)?;
    if !body.vel_rad_s.is_finite() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "bad_request",
            Some("vel_rad_s must be finite".into()),
        ));
    }
    let vel = clamp_speed(body.vel_rad_s);
    let intent = MotionIntent::Jog { vel_rad_s: vel };

    // Hot path: already-running jog → just push the new intent (which
    // also refreshes the heartbeat in the controller).
    if let Some(snap) = state.motion.current(&role) {
        if matches!(snap.kind.as_str(), "jog") {
            state.motion.update_intent(&role, intent.clone());
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: session,
                remote: None,
                action: "motion_jog_heartbeat".into(),
                target: Some(role.clone()),
                details: serde_json::json!({
                    "vel_rad_s": vel,
                    "run_id": snap.run_id,
                }),
                result: AuditResult::Ok,
            });
            return Ok(Json(StartResp {
                run_id: snap.run_id,
                clamped_speed_rad_s: vel,
            }));
        }
    }

    start(&state, session, &role, intent, vel).await
}

/// POST /api/motors/:role/motion/stop
///
/// Idempotent: returns `{stopped: false}` if the role had no active
/// motion. Always 200; failures only happen if the lock check fails.
pub async fn stop(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(role): Path<String>,
) -> Result<Json<StopResp>, (StatusCode, Json<ApiError>)> {
    let session = ensure_lock(&state, &headers)?;
    let stopped = state.motion.stop(&role).await;
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "motion_stop_request".into(),
        target: Some(role.clone()),
        details: serde_json::json!({ "had_active": stopped }),
        result: AuditResult::Ok,
    });
    Ok(Json(StopResp { stopped }))
}

/// Common start path used by sweep / wave / jog (when no jog is
/// already running). Audit-logs the request and translates preflight
/// failures into the right HTTP envelope.
async fn start(
    state: &SharedState,
    session: Option<String>,
    role: &str,
    intent: MotionIntent,
    clamped_speed: f32,
) -> Result<Json<StartResp>, (StatusCode, Json<ApiError>)> {
    match state.motion.start(state, role, intent.clone()).await {
        Ok(run_id) => {
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: session,
                remote: None,
                action: "motion_start_request".into(),
                target: Some(role.to_string()),
                details: serde_json::json!({
                    "run_id": run_id,
                    "kind": intent.kind_str(),
                    "intent": serde_json::to_value(&intent)
                        .unwrap_or(serde_json::Value::Null),
                }),
                result: AuditResult::Ok,
            });
            Ok(Json(StartResp {
                run_id,
                clamped_speed_rad_s: clamped_speed,
            }))
        }
        Err(e) => {
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: session,
                remote: None,
                action: "motion_start_request".into(),
                target: Some(role.to_string()),
                details: serde_json::json!({
                    "kind": intent.kind_str(),
                    "error": e.code(),
                    "detail": e.detail(role),
                }),
                result: AuditResult::Denied,
            });
            Err(preflight_to_http(role, e))
        }
    }
}
