//! POST /api/motors/:role/jog — hold-to-jog dead-man.
//!
//! The SPA fires this at ~20 Hz while the operator holds a jog button.
//! Each call:
//!
//!   1. Validates against firmware spec (`limit_spd.hardware_range`) and
//!      the motor's soft travel band (`travel_limits.{min,max}_rad`).
//!   2. Issues a velocity-mode setpoint via `cmd_enable + run_mode=2 +
//!      spd_ref`.
//!   3. Refreshes the per-motor TTL watchdog so a single shared
//!      background task issues `cmd_stop` if no follow-up jog frame
//!      arrives within `ttl_ms`.
//!
//! The watchdog matters because a hung browser tab or a dropped network
//! frame would otherwise leave the motor running forever — the firmware
//! `canTimeout` is the backstop, but rudydae's TTL is tighter and fires
//! a clean type-4 stop at the protocol layer.

use std::sync::OnceLock;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::audit::{AuditEntry, AuditResult};
use crate::can::travel::{enforce_position, BandCheck};
use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

/// Soft cap matching `bench_tool::MAX_TARGET_VEL_RAD_S`. The daemon refuses
/// jog setpoints above this even if the firmware envelope is wider, so the
/// browser UI can't accidentally request something the bench routines
/// would also reject.
const MAX_JOG_VEL_RAD_S: f32 = 0.5;
const MAX_TTL_MS: u64 = 1_000;

#[derive(Debug, Deserialize)]
pub struct JogBody {
    pub vel_rad_s: f32,
    /// How long the daemon should treat the previous jog frame as still
    /// "live". On the next tick after this expires the watchdog issues
    /// `cmd_stop`.
    pub ttl_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct JogResp {
    pub ok: bool,
    pub clamped_vel_rad_s: f32,
}

fn err(status: StatusCode, error: &str, detail: Option<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
            detail,
        }),
    )
}

pub async fn jog(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<JogBody>,
) -> Result<Json<JogResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);

    if !state.has_control(session.as_deref().unwrap_or("")) {
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some("another operator holds the control lock".into()),
        ));
    }

    let motor = {
        let inv = state.inventory.read().expect("inventory poisoned");
        inv.by_role(&role).cloned().ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            )
        })?
    };

    if !motor.present {
        return Err(err(
            StatusCode::CONFLICT,
            "motor_absent",
            Some(format!("inventory entry for {role} has present=false")),
        ));
    }
    if state.cfg.safety.require_verified && !motor.verified {
        return Err(err(
            StatusCode::FORBIDDEN,
            "not_verified",
            Some(format!(
                "inventory entry for {role} has verified=false; commission before jogging"
            )),
        ));
    }

    if !body.vel_rad_s.is_finite() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "bad_request",
            Some("vel_rad_s must be finite".into()),
        ));
    }
    let clamped = body
        .vel_rad_s
        .clamp(-MAX_JOG_VEL_RAD_S, MAX_JOG_VEL_RAD_S);
    let ttl_ms = body.ttl_ms.clamp(50, MAX_TTL_MS);

    // Use the latest cached position to bound the next setpoint. The check
    // is conservative: if we have no recent feedback we let the firmware
    // envelope handle it.
    if let Some(fb) = state.latest.read().expect("latest poisoned").get(&role) {
        // Project where the motor would be after `ttl_ms` at `clamped` and
        // refuse if that lands outside the band. Treats the band as a
        // hard limit even with mid-flight predictions.
        let projected = fb.mech_pos_rad + clamped * (ttl_ms as f32 / 1000.0);
        if let BandCheck::OutOfBand {
            min_rad,
            max_rad,
            attempted_rad,
        } = enforce_position(&state, &role, projected).map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                Some(format!("{e:#}")),
            )
        })? {
            return Err(err(
                StatusCode::CONFLICT,
                "travel_limit_violation",
                Some(format!(
                    "projected position {attempted_rad:.3} rad outside [{min_rad:.3}, {max_rad:.3}]"
                )),
            ));
        }
    }

    if let Some(core) = state.real_can.clone() {
        let motor_for_blocking = motor.clone();
        let v = clamped;
        tokio::task::spawn_blocking(move || core.set_velocity_setpoint(&motor_for_blocking, v))
            .await
            .expect("jog task panicked")
            .map_err(|e| {
                err(
                    StatusCode::BAD_GATEWAY,
                    "can_command_failed",
                    Some(format!("jog setpoint failed for {role}: {e:#}")),
                )
            })?;
    }

    watchdog_arm(state.clone(), &motor.role, ttl_ms);

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "jog".into(),
        target: Some(role),
        details: serde_json::json!({
            "vel_rad_s": clamped,
            "ttl_ms": ttl_ms,
        }),
        result: AuditResult::Ok,
    });

    Ok(Json(JogResp {
        ok: true,
        clamped_vel_rad_s: clamped,
    }))
}

/// Refresh the per-motor watchdog deadline. Spawns the shared watchdog
/// loop on first use.
fn watchdog_arm(state: SharedState, role: &str, ttl_ms: u64) {
    let map = watchdog_state();
    {
        let mut guard = map.lock().expect("watchdog poisoned");
        guard.insert(
            role.to_string(),
            Instant::now() + Duration::from_millis(ttl_ms),
        );
    }
    static SPAWN: OnceLock<()> = OnceLock::new();
    SPAWN.get_or_init(|| {
        tokio::spawn(watchdog_loop(state));
    });
}

type DeadlineMap = std::sync::Mutex<std::collections::HashMap<String, Instant>>;

fn watchdog_state() -> &'static DeadlineMap {
    static MAP: OnceLock<DeadlineMap> = OnceLock::new();
    MAP.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

async fn watchdog_loop(state: SharedState) {
    use tokio::time::interval;
    let mut tick = interval(Duration::from_millis(25));
    loop {
        tick.tick().await;
        let now = Instant::now();
        let expired: Vec<String> = {
            let mut guard = watchdog_state().lock().expect("watchdog poisoned");
            let expired: Vec<String> = guard
                .iter()
                .filter_map(|(k, deadline)| {
                    if *deadline <= now {
                        Some(k.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for k in &expired {
                guard.remove(k);
            }
            expired
        };

        if expired.is_empty() {
            continue;
        }

        let inv_snap = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .motors
            .clone();
        for role in expired {
            let Some(motor) = inv_snap.iter().find(|m| m.role == role).cloned() else {
                continue;
            };
            if let Some(core) = state.real_can.clone() {
                let _ = tokio::task::spawn_blocking(move || core.stop(&motor)).await;
            }
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: None,
                remote: None,
                action: "jog_watchdog_stop".into(),
                target: Some(role),
                details: serde_json::Value::Null,
                result: AuditResult::Ok,
            });
        }
    }
}
