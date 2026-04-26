//! POST /api/restart — operator-triggered daemon restart.
//!
//! Motivation: after a deploy lands (the Pi runs `cortex-update.timer` every
//! 60s), the operator often wants to *force* the new build to take effect
//! immediately rather than waiting for the next watchdog or natural restart.
//! This endpoint asks the daemon to drop torque on every motor and exit;
//! `Restart=always` in `cortex.service` brings the new binary back up under
//! systemd within `RestartSec=3`. In `npm run dev` (no supervisor) the
//! operator must restart `cortex` manually — the JSON envelope advertises
//! `supervised: false` for that case so the SPA can adjust copy.
//!
//! The bus-state shutdown mirrors `/api/estop`: every motor we know to be
//! enabled gets a `cmd_stop` so torque doesn't outlive the process. Failures
//! are best-effort (per-motor), the same posture as estop. The audit log
//! records the request before the exit is scheduled.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use serde::Serialize;

use crate::audit::{AuditEntry, AuditResult};
use crate::inventory::Actuator;
use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

/// Delay between flushing the response and `process::exit`. Sized for the
/// HTTP write to drain on loopback (~ms) plus a bit of buffer for the WT
/// router's safety-event broadcast to land on subscribers; well under
/// `cortex.service`'s `RestartSec=3` so the systemd-driven restart still
/// dominates the perceived downtime.
const EXIT_DELAY_MS: u64 = 500;

#[derive(Debug, Serialize)]
pub struct RestartResp {
    pub ok: bool,
    /// Number of motors we issued `cmd_stop` to before the exit was queued.
    pub stopped: usize,
    /// Milliseconds the daemon will wait before calling `process::exit(0)`.
    /// The SPA uses this to size its "Restarting…" countdown.
    pub restart_in_ms: u64,
    /// `true` when the daemon is running under a supervisor that will
    /// auto-restart it (systemd on Linux). `false` for `npm run dev` /
    /// `cargo run` setups where the operator must restart by hand.
    pub supervised: bool,
}

pub async fn restart(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Result<(StatusCode, Json<RestartResp>), (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);

    let _motion_tasks_stopped = state.motion.stop_all().await;

    // Best-effort drop-torque pass before exit. We fan to *every present*
    // motor (not just `enabled`) because the in-memory enabled-set is
    // best-effort and we'd rather send an idempotent stop frame to a
    // motor that was already stopped than miss one that drifted out of
    // sync with our bookkeeping. Same posture as `/api/estop`.
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
                if core.stop(motor).is_ok() {
                    stopped.push(motor.common.role.clone());
                }
            }
            stopped
        })
        .await
        .expect("restart stop task panicked")
    } else {
        motors.iter().map(|m| m.common.role.clone()).collect()
    };

    for role in &stopped {
        state.mark_stopped(role);
    }
    let stopped = stopped.len();

    let supervised = is_supervised();

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session.clone(),
        remote: None,
        action: "restart_requested".into(),
        target: None,
        details: serde_json::json!({
            "stopped": stopped,
            "total": motors.len(),
            "exit_delay_ms": EXIT_DELAY_MS,
            "supervised": supervised,
        }),
        result: AuditResult::Ok,
    });

    tracing::warn!(
        stopped,
        total = motors.len(),
        supervised,
        session = ?session,
        "restart requested; daemon will exit in {EXIT_DELAY_MS}ms"
    );

    // Skip the actual exit when an integration test has flipped the
    // suppression flag. `cortex::build_app` is what tests build against
    // and they hit handlers through `tower::ServiceExt::oneshot`, so the
    // handler runs in the test process; calling `process::exit(0)` would
    // tear the test runner down with it.
    if !SUPPRESS_EXIT.load(Ordering::SeqCst) {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(EXIT_DELAY_MS)).await;
            // SIGTERM-style clean exit. systemd treats exit code 0 with
            // `Restart=always` as a normal restart trigger; the HTTP
            // response from this handler has already flushed by now.
            std::process::exit(0);
        });
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(RestartResp {
            ok: true,
            stopped,
            restart_in_ms: EXIT_DELAY_MS,
            supervised,
        }),
    ))
}

/// Suppress the actual `process::exit(0)` call inside [`restart`].
/// Integration tests that exercise the handler via `cortex::build_app`
/// flip this once at startup so the handler returns its envelope without
/// taking the `cargo test` runner down. Production callers should never
/// touch it.
static SUPPRESS_EXIT: AtomicBool = AtomicBool::new(false);

/// Test-only: disable the trailing `process::exit(0)` in the restart
/// handler. Idempotent. Once flipped, never reset.
pub fn suppress_exit_for_tests() {
    SUPPRESS_EXIT.store(true, Ordering::SeqCst);
}

/// Best-effort detection of "running under a supervisor that will restart
/// us on exit." On Linux/systemd `INVOCATION_ID` is set in the unit's
/// environment by systemd itself, so its presence is a reliable hint.
/// Everywhere else we conservatively report `false` so the SPA's copy
/// reads "you'll need to restart cortex manually."
fn is_supervised() -> bool {
    std::env::var("INVOCATION_ID")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}
