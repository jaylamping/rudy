//! REST contract tests — pin every endpoint the `link/` SPA calls.
//!
//! For each endpoint in `link/src/lib/api.ts` we:
//!   1. Build the same `axum::Router` `rudydae` serves (via `rudydae::build_app`).
//!   2. Issue an in-process request with `tower::ServiceExt::oneshot` so we
//!      don't depend on a real TCP socket / TLS cert / port allocation.
//!   3. Deserialize the body into the **exact** Rust type that `ts-rs` exports
//!      to TypeScript. Any drift between the Rust struct and the response shape
//!      blows up the test.
//!
//! These tests do not run against real CAN hardware. They use the mock CAN
//! core (no socket open) and seed `state.latest` / `state.params` directly so
//! the "no feedback yet" race is not a flake source.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use rudydae::types::{
    ApiError, MotorFeedback, MotorSummary, ParamSnapshot, Reminder, ServerConfig, ServerFeatures,
    SystemSnapshot, WebTransportAdvert,
};

mod common;

async fn body_json<T: serde::de::DeserializeOwned>(resp: axum::response::Response) -> T {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<T>(&bytes).unwrap_or_else(|e| {
        let s = std::str::from_utf8(&bytes).unwrap_or("<binary>");
        panic!("deserialise failed: {e}; body was: {s}");
    })
}

#[tokio::test]
async fn get_config_returns_server_config_shape() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let cfg: ServerConfig = body_json(resp).await;

    // Every TS-exported field must be populated.
    assert!(!cfg.version.is_empty(), "version must be present");
    assert_eq!(cfg.actuator_model, "TEST_RS03");

    // Disabled-WT advert: enabled=false AND url=None — the SPA's
    // useWebTransport hook reads exactly this shape.
    let WebTransportAdvert { enabled, url } = cfg.webtransport;
    assert!(!enabled);
    assert!(url.is_none());

    let ServerFeatures {
        mock_can,
        require_verified,
    } = cfg.features;
    assert!(mock_can);
    assert!(require_verified);
}

/// When WT is enabled, `/api/config` advertises a URL the browser opens
/// directly. The host is taken from the inbound `Host` header (which is what
/// the browser already resolved to reach the SPA — on the Pi this is the
/// tailnet hostname forwarded through `tailscale serve`); the port comes
/// from `[webtransport].bind`.
#[tokio::test]
async fn get_config_advertises_resolvable_wt_url_when_enabled() {
    let (state, _dir) = common::make_state_with_wt_advert();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .header("host", "rudy-pi.tail0b414.ts.net")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cfg: ServerConfig = body_json(resp).await;

    assert!(cfg.webtransport.enabled);
    let url = cfg.webtransport.url.expect("WT URL when enabled");
    assert!(
        !url.contains("HOSTPLACEHOLDER"),
        "config_route should substitute the host before responding; got {url}"
    );
    assert!(
        url.starts_with("https://"),
        "WT URL must be https for the browser to accept it; got {url}"
    );
    assert_eq!(
        url, "https://rudy-pi.tail0b414.ts.net:4433/wt",
        "WT URL should reuse the inbound Host hostname (sans :port) and the WT bind port",
    );
}

/// `Host` headers from the browser often include a `:port` (e.g. dev servers
/// bound to `:5173`). We must strip it before reattaching the WT port,
/// otherwise we'd advertise `https://localhost:5173:4433/wt` which the
/// browser refuses to parse.
#[tokio::test]
async fn get_config_strips_port_from_host_header() {
    let (state, _dir) = common::make_state_with_wt_advert();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .header("host", "localhost:5173")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cfg: ServerConfig = body_json(resp).await;
    let url = cfg.webtransport.url.expect("WT URL when enabled");
    assert_eq!(url, "https://localhost:4433/wt");
}

/// If no `Host` header is present we cannot synthesise a URL the browser can
/// resolve, so we omit it. The frontend treats `enabled=true, url=None` the
/// same as disabled (no WT session opens) rather than crashing on a bad URL.
#[tokio::test]
async fn get_config_omits_wt_url_when_host_header_missing() {
    let (state, _dir) = common::make_state_with_wt_advert();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cfg: ServerConfig = body_json(resp).await;
    assert!(cfg.webtransport.enabled);
    assert!(cfg.webtransport.url.is_none());
}

#[tokio::test]
async fn list_motors_matches_motor_summary() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    let app = rudydae::build_app(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let motors: Vec<MotorSummary> = body_json(resp).await;
    assert_eq!(motors.len(), state.inventory.motors.len());

    let by_role: std::collections::BTreeMap<&str, &MotorSummary> =
        motors.iter().map(|m| (m.role.as_str(), m)).collect();
    let a = by_role
        .get("shoulder_actuator_a")
        .expect("shoulder_actuator_a present");
    assert_eq!(a.can_id, 0x08);
    assert_eq!(a.can_bus, "can1");
    assert!(a.verified);
    assert!(a.latest.is_some(), "we just seeded feedback");

    let b = by_role
        .get("shoulder_actuator_b")
        .expect("shoulder_actuator_b present");
    assert!(!b.verified);
}

#[tokio::test]
async fn get_motor_404_returns_api_error_envelope() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "unknown_motor");
    assert!(err.detail.is_some(), "404s should explain themselves");
}

#[tokio::test]
async fn get_feedback_returns_motor_feedback_shape() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/feedback")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let fb: MotorFeedback = body_json(resp).await;
    assert_eq!(fb.role, "shoulder_actuator_a");
    assert_eq!(fb.can_id, 0x08);
    // i64 millisecond timestamp — TS sees it as bigint per ts-rs, so changes
    // here ripple into useWebTransport / TelemetryGrid.
    assert!(fb.t_ms > 0);
}

#[tokio::test]
async fn get_feedback_404_when_no_telemetry_yet() {
    let (state, _dir) = common::make_state();
    // Note: deliberately NOT seeding feedback.
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/feedback")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "no_feedback");
}

#[tokio::test]
async fn get_params_returns_param_snapshot_shape() {
    let (state, _dir) = common::make_state();
    common::seed_params(&state);
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/params")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let snap: ParamSnapshot = body_json(resp).await;
    assert_eq!(snap.role, "shoulder_actuator_a");

    let lt = snap
        .values
        .get("limit_torque")
        .expect("limit_torque must be in the snapshot from spec");
    assert_eq!(lt.index, 0x700B);
    assert_eq!(lt.ty, "float");
    assert_eq!(lt.hardware_range, Some([0.0, 60.0]));
    assert_eq!(lt.units.as_deref(), Some("nm"));
}

#[tokio::test]
async fn put_param_in_range_returns_write_envelope() {
    let (state, _dir) = common::make_state();
    common::seed_params(&state);
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({ "value": 12.5, "save_after": false })).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/params/limit_torque")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Frontend literally destructures these fields in api.writeParam().
    #[derive(serde::Deserialize)]
    struct WriteResp {
        ok: bool,
        saved: bool,
        role: String,
        name: String,
        value: serde_json::Value,
    }
    let r: WriteResp = body_json(resp).await;
    assert!(r.ok);
    assert!(!r.saved);
    assert_eq!(r.role, "shoulder_actuator_a");
    assert_eq!(r.name, "limit_torque");
    assert_eq!(r.value, json!(12.5));
}

#[tokio::test]
async fn put_param_out_of_range_returns_api_error() {
    let (state, _dir) = common::make_state();
    common::seed_params(&state);
    let app = rudydae::build_app(state);

    // hardware_range is [0.0, 60.0]; 9999 is well outside.
    let body = serde_json::to_vec(&json!({ "value": 9999.0, "save_after": false })).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/params/limit_torque")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_range");
}

#[tokio::test]
async fn put_param_unknown_param_404s() {
    let (state, _dir) = common::make_state();
    common::seed_params(&state);
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({ "value": 1.0 })).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/params/no_such_param")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "unknown_param");
}

/// All four control endpoints share the same `{ ok, role }` envelope. The
/// frontend's `api.enable / stop / saveToFlash / setZero` mutations expect
/// `{ ok: true }` at minimum — extra fields are tolerated.
#[tokio::test]
async fn control_endpoints_return_ok_envelope_for_verified_motor() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    for (verb, suffix) in [
        ("POST", "enable"),
        ("POST", "stop"),
        ("POST", "save"),
        ("POST", "set_zero"),
    ] {
        let req = Request::builder()
            .method(verb)
            .uri(format!("/api/motors/shoulder_actuator_a/{suffix}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "{verb} /api/motors/shoulder_actuator_a/{suffix} should succeed for a verified motor"
        );
        let v: serde_json::Value = body_json(resp).await;
        assert_eq!(
            v.get("ok").and_then(|b| b.as_bool()),
            Some(true),
            "control envelope must include ok:true; got {v}"
        );
    }
}

/// Unverified motor + require_verified=true => 403 from `enable`. Mirrors the
/// safety gate in `control::enable` and confirms the SPA can rely on the 403
/// to keep the button locked out.
#[tokio::test]
async fn enable_unverified_motor_is_forbidden() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_b/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "not_verified");
}

/// `GET /api/system` returns a `SystemSnapshot`. With `cfg.can.mock = true`
/// (the test fixture's default) the snapshot is mocked: `is_mock=true`, all
/// numeric fields populated. Pins the wire shape the dashboard's
/// `SystemHealthCard` consumes.
#[tokio::test]
async fn get_system_returns_system_snapshot_shape() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/system")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let snap: SystemSnapshot = body_json(resp).await;
    assert!(snap.is_mock, "test fixture has cfg.can.mock=true");
    assert!(snap.cpu_pct >= 0.0 && snap.cpu_pct <= 100.0);
    assert!(snap.mem_total_mb > 0);
    assert!(snap.t_ms > 0);
    assert_eq!(snap.load.len(), 3);
}

/// Reminders CRUD: empty list on first call, create returns 201 + reminder
/// echoed back, list reflects it, update mutates fields, delete returns 204.
/// One test pinning the whole flow because the operations only make sense
/// in sequence (id from create -> update/delete).
#[tokio::test]
async fn reminders_crud_roundtrip() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

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
    let app = rudydae::build_app(state);

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

/// Sanity check: the URL paths the test hits above are exactly the ones the
/// SPA's `link/src/lib/api.ts` constructs. If someone renames a route in
/// `crates/rudydae/src/api/mod.rs`, this test file fails to compile because
/// the URI constants would still point at the old shape — but a harder pin
/// lives on the Node smoke runner under `link/scripts/smoke-contract.mjs`.
#[test]
fn endpoint_inventory_documented() {
    // Single source of truth listing every route hit by the SPA today. When
    // adding a new route to `link/src/lib/api.ts`, add it here too AND write a
    // contract test above. The string is deliberately not parsed — it's a
    // checklist for code reviewers.
    let _spa_endpoints = [
        "GET    /api/config",
        "GET    /api/system",
        "GET    /api/motors",
        "GET    /api/motors/:role",
        "GET    /api/motors/:role/feedback",
        "GET    /api/motors/:role/params",
        "PUT    /api/motors/:role/params/:name",
        "POST   /api/motors/:role/enable",
        "POST   /api/motors/:role/stop",
        "POST   /api/motors/:role/save",
        "POST   /api/motors/:role/set_zero",
        "GET    /api/reminders",
        "POST   /api/reminders",
        "PUT    /api/reminders/:id",
        "DELETE /api/reminders/:id",
    ];
}
