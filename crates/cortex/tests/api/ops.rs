//! Reminders, estop, control lock.
//!
#![allow(unused_imports)]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use cortex::inventory::{Device, Inventory, TravelLimits};
use cortex::types::{
    ApiError, MotorFeedback, MotorSummary, ParamSnapshot, Reminder, SafetyEvent, ServerConfig,
    ServerFeatures, SystemSnapshot, WebTransportAdvert,
};

#[path = "../common/mod.rs"]
mod common;
use common::body_json;

/// Reminders CRUD: empty list on first call, create returns 201 + reminder
/// echoed back, list reflects it, update mutates fields, delete returns 204.
/// One test pinning the whole flow because the operations only make sense
/// in sequence (id from create -> update/delete).
#[tokio::test]
async fn reminders_crud_roundtrip() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    // Empty initial list.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/reminders")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let initial: Vec<Reminder> = body_json(resp).await;
    assert!(initial.is_empty(), "fresh tempdir should have no reminders");

    // Create.
    let body = serde_json::to_vec(&json!({ "text": "torque rs03" })).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/reminders")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Reminder = body_json(resp).await;
    assert_eq!(created.text, "torque rs03");
    assert!(!created.done);
    assert!(!created.id.is_empty());

    // List shows the created one.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/reminders")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let listed: Vec<Reminder> = body_json(resp).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);

    // Update done=true.
    let body = serde_json::to_vec(&json!({ "text": created.text, "done": true })).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/reminders/{}", created.id))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated: Reminder = body_json(resp).await;
    assert!(updated.done);

    // Delete.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/reminders/{}", created.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 404 on the now-missing id.
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/reminders/{}", created.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Empty text is rejected with 400 / `empty_text` so the SPA can show a
/// friendly inline validation error without parsing free-form messages.
#[tokio::test]
async fn create_reminder_with_blank_text_400s() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({ "text": "   " })).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/reminders")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "empty_text");
}

/// `GET /api/motors/:role/travel_limits` returns 404 + `no_travel_limits`
/// when no band has been configured yet. This is the SPA's signal to fall
/// back to "use spec defaults" instead of erroring loudly.
#[tokio::test]
async fn estop_returns_ok_envelope() {
    let (state, _dir) = common::make_state();
    let total_present = state
        .inventory
        .read()
        .expect("inventory")
        .actuators()
        .filter(|m| m.common.present)
        .count();
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/estop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    #[derive(serde::Deserialize)]
    struct Resp {
        ok: bool,
        stopped: usize,
    }
    let r: Resp = body_json(resp).await;
    assert!(r.ok);
    assert_eq!(r.stopped, total_present);
}

/// `POST /api/restart` returns a 202 envelope (the daemon would exit
/// asynchronously in production), confirms it stopped every present motor
/// in the test inventory, and surfaces the `supervised` hint. The exit
/// itself is suppressed by [`cortex::api::ops::restart::suppress_exit_for_tests`]
/// — otherwise the spawned `process::exit(0)` would tear the test runner
/// down 500ms into the run.
#[tokio::test]
async fn restart_returns_accepted_envelope() {
    cortex::api::ops::restart::suppress_exit_for_tests();

    let (state, _dir) = common::make_state();
    let total_present = state
        .inventory
        .read()
        .expect("inventory")
        .actuators()
        .filter(|m| m.common.present)
        .count();
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/restart")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    #[derive(serde::Deserialize)]
    struct Resp {
        ok: bool,
        stopped: usize,
        restart_in_ms: u64,
        #[allow(dead_code)]
        supervised: bool,
    }
    let r: Resp = body_json(resp).await;
    assert!(r.ok);
    assert_eq!(r.stopped, total_present);
    assert!(
        r.restart_in_ms > 0,
        "should advertise a non-zero exit delay"
    );
}

/// First mutator from a fresh session implicitly claims the control lock,
/// and a *second* concurrent session is then refused with 423 Locked.
///
/// This is the only failure mode the lock exists to guard against on a
/// solo-operator deployment (two browser tabs racing each other on the bus).
/// There is no `/api/lock` endpoint and no UI: the gate is invisible until
/// it bites a stale tab.
#[tokio::test]
async fn second_session_rejected_after_first_implicitly_claims_lock() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    // session-A makes a normal mutating call. This succeeds AND silently
    // promotes session-A to lock holder.
    let body = serde_json::to_vec(&json!({"min_rad": -0.5, "max_rad": 0.5})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .header("content-type", "application/json")
                .header("x-rudy-session", "session-A")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // session-B's mutator is now refused with the holder's id surfaced in
    // the detail string (so a debugger can see *which* tab is in the way).
    let body = serde_json::to_vec(&json!({"min_rad": -0.4, "max_rad": 0.4})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .header("content-type", "application/json")
                .header("x-rudy-session", "session-B")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::from_u16(423).unwrap());
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "lock_held");
    assert!(
        err.detail.as_deref().unwrap_or("").contains("session-A"),
        "detail should name the holder: {:?}",
        err.detail
    );
}
