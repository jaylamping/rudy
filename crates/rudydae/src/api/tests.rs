//! POST /api/motors/:role/tests/:name — run one bench routine.
//!
//! Returns a `run_id` immediately; the routine itself runs on a background
//! thread and emits `TestProgress` frames over the WebTransport
//! `test_progress` stream so the SPA can render a live log.
//!
//! Single-operator-locked + audit-logged + non-Linux-safe (mock CAN simply
//! emits a synthetic `pass` line so the SPA still feels exercised in dev).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use uuid::Uuid;

use crate::audit::{AuditEntry, AuditResult};
use crate::state::SharedState;
use crate::types::{ApiError, TestLevel, TestName, TestProgress};
use crate::util::session_from_headers;

#[derive(Debug, Deserialize, Default)]
pub struct TestsBody {
    /// `set_zero`: also issue type-22 save after the zero.
    #[serde(default)]
    pub save: bool,
    /// `jog`: target velocity (rad/s).
    #[serde(default)]
    pub target_vel: Option<f32>,
    /// `jog`: duration (s).
    #[serde(default)]
    pub duration: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct TestsResp {
    pub run_id: String,
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

pub async fn run_test(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path((role, name)): Path<(String, String)>,
    Json(body): Json<TestsBody>,
) -> Result<Json<TestsResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);

    if !state.has_control(session.as_deref().unwrap_or("")) {
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some("another operator holds the control lock".into()),
        ));
    }

    let test = parse_name(&name).ok_or_else(|| {
        err(
            StatusCode::BAD_REQUEST,
            "unknown_test",
            Some(format!("no bench routine named {name}")),
        )
    })?;

    // Cheap synchronous validity checks before we spawn anything.
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

    let run_id = Uuid::new_v4().to_string();

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session.clone(),
        remote: None,
        action: format!("test_{}", test.as_str()),
        target: Some(role.clone()),
        details: serde_json::json!({
            "run_id": run_id,
            "save": body.save,
            "target_vel": body.target_vel,
            "duration": body.duration,
        }),
        result: AuditResult::Ok,
    });

    spawn_runner(state.clone(), test, role, run_id.clone(), body, motor);

    Ok(Json(TestsResp { run_id }))
}

fn parse_name(s: &str) -> Option<TestName> {
    match s {
        "read" => Some(TestName::Read),
        "set_zero" => Some(TestName::SetZero),
        "smoke" => Some(TestName::Smoke),
        "jog" => Some(TestName::Jog),
        "jog_overlimit" => Some(TestName::JogOverlimit),
        _ => None,
    }
}

fn spawn_runner(
    state: SharedState,
    test: TestName,
    role: String,
    run_id: String,
    body: TestsBody,
    motor: crate::inventory::Motor,
) {
    tokio::spawn(async move {
        let seq = std::sync::Arc::new(AtomicU64::new(0));
        let emit = |level: TestLevel, step: &str, message: &str| {
            let p = TestProgress {
                run_id: run_id.clone(),
                role: role.clone(),
                seq: seq.fetch_add(1, Ordering::SeqCst),
                t_ms: Utc::now().timestamp_millis(),
                step: step.to_string(),
                level,
                message: message.to_string(),
            };
            let _ = state.test_progress_tx.send(p);
        };

        emit(
            TestLevel::Info,
            "spawn",
            &format!(
                "starting {} on {} (run_id={run_id})",
                test.as_str(),
                role
            ),
        );

        if state.real_can.is_none() {
            // Mock mode: synthesize a quick pass so the SPA wiring is
            // exercisable in dev without hardware.
            emit(
                TestLevel::Info,
                "mock",
                "no real CAN core; synthesising a mock pass",
            );
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            emit(
                TestLevel::Pass,
                "done",
                "mock CAN routine completed (no actual CAN frames sent)",
            );
            return;
        }

        // Real CAN: hand off to the blocking pool. The Linux core owns the
        // bus exclusively so the routine takes the per-iface mutex for its
        // entire duration; that's what keeps the telemetry poller from
        // racing the routine on the same wire.
        run_real(state.clone(), test, motor, body, &run_id, &role, seq).await;
    });
}

#[cfg(target_os = "linux")]
async fn run_real(
    state: SharedState,
    test: TestName,
    motor: crate::inventory::Motor,
    body: TestsBody,
    run_id: &str,
    role: &str,
    seq: std::sync::Arc<AtomicU64>,
) {
    use driver::rs03::tests::{self as bench, Common, Level, Reporter, RoutineOutcome};
    use std::sync::atomic::AtomicBool;

    struct WtReporter {
        tx: tokio::sync::broadcast::Sender<TestProgress>,
        run_id: String,
        role: String,
        seq: std::sync::Arc<AtomicU64>,
    }

    impl Reporter for WtReporter {
        fn report(&mut self, step: &str, level: Level, message: &str) {
            let p = TestProgress {
                run_id: self.run_id.clone(),
                role: self.role.clone(),
                seq: self.seq.fetch_add(1, Ordering::SeqCst),
                t_ms: Utc::now().timestamp_millis(),
                step: step.to_string(),
                level: match level {
                    Level::Info => TestLevel::Info,
                    Level::Warn => TestLevel::Warn,
                    Level::Pass => TestLevel::Pass,
                    Level::Fail => TestLevel::Fail,
                },
                message: message.to_string(),
            };
            let _ = self.tx.send(p);
        }
    }

    let role_owned = role.to_string();
    let run_id_owned = run_id.to_string();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut reporter = WtReporter {
            tx: state.test_progress_tx.clone(),
            run_id: run_id_owned,
            role: role_owned,
            seq,
        };
        let common = Common {
            host_id: 0xFD,
            motor_id: motor.can_id,
        };
        let stop = AtomicBool::new(false);

        // Dispatch via the LinuxCanCore's existing per-bus mutex so the
        // bench routine doesn't race the telemetry poller for the same
        // socket. This pattern matches `LinuxCanCore::with_bus`; we route
        // through a private helper that exposes the bus to the closure.
        let core = state.real_can.clone().expect("real_can present");
        match test {
            TestName::Read => core.with_bus_for_test(&motor.can_bus, |bus| {
                bench::run_read(bus, &common, &mut reporter)
            }),
            TestName::SetZero => core.with_bus_for_test(&motor.can_bus, |bus| {
                bench::run_set_zero(bus, &common, body.save, &mut reporter)
            }),
            TestName::Smoke => core.with_bus_for_test(&motor.can_bus, |bus| {
                bench::run_smoke(bus, &common, true, &stop, &mut reporter)
            }),
            TestName::Jog => core.with_bus_for_test(&motor.can_bus, |bus| {
                bench::run_jog(
                    bus,
                    &common,
                    body.target_vel.unwrap_or(0.2),
                    body.duration.unwrap_or(2.0),
                    true,
                    false,
                    &stop,
                    &mut reporter,
                )
            }),
            TestName::JogOverlimit => core.with_bus_for_test(&motor.can_bus, |bus| {
                bench::run_jog(
                    bus,
                    &common,
                    0.0,
                    1.0,
                    true,
                    true,
                    &stop,
                    &mut reporter,
                )
            }),
        }
    })
    .await
    .expect("bench task panicked");

    // Surface any io::Error from the with_bus_for_test helper itself
    // (e.g. iface not configured). The bench routines themselves emit
    // `Fail` lines through the reporter for protocol-level failures.
    if let Err(e) = outcome {
        let p = TestProgress {
            run_id: run_id.to_string(),
            role: role.to_string(),
            seq: 0,
            t_ms: Utc::now().timestamp_millis(),
            step: "spawn".to_string(),
            level: TestLevel::Fail,
            message: format!("bench harness error: {e:#}"),
        };
        let _ = state.test_progress_tx.send(p);
    } else if let Ok(RoutineOutcome::Fail(rc)) = outcome {
        // Outcome already emitted its own Fail line via the reporter, but
        // pin the rc on the audit log so post-mortem JSON queries can find
        // it without parsing free-form messages.
        state.audit.write(AuditEntry {
            timestamp: Utc::now(),
            session_id: None,
            remote: None,
            action: format!("test_{}_fail", test.as_str()),
            target: Some(role.to_string()),
            details: serde_json::json!({"run_id": run_id, "rc": rc}),
            result: AuditResult::Error,
        });
    }
}

#[cfg(not(target_os = "linux"))]
async fn run_real(
    state: SharedState,
    _test: TestName,
    _motor: crate::inventory::Motor,
    _body: TestsBody,
    run_id: &str,
    role: &str,
    seq: std::sync::Arc<AtomicU64>,
) {
    let p = TestProgress {
        run_id: run_id.to_string(),
        role: role.to_string(),
        seq: seq.fetch_add(1, Ordering::SeqCst),
        t_ms: Utc::now().timestamp_millis(),
        step: "platform".to_string(),
        level: TestLevel::Fail,
        message: "real bench routines require Linux + SocketCAN".to_string(),
    };
    let _ = state.test_progress_tx.send(p);
}
