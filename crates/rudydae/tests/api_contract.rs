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

use rudydae::inventory::TravelLimits;
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
    assert_eq!(
        motors.len(),
        state
            .inventory
            .read()
            .expect("inventory poisoned")
            .motors
            .len()
    );

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
    // Pre-date the boot-time gate: this test only cares about the envelope
    // shape, not the gate. Force every motor to Homed so enable doesn't
    // trip the new ritual checks.
    common::force_homed(&state);
    let app = rudydae::build_app(state);

    // (verb, suffix, body) triples. `set_zero` is the only one that
    // requires a body (the `confirm_advanced: true` opt-in flag);
    // every other control endpoint is bodyless.
    for (verb, suffix, body_json_str) in [
        ("POST", "enable", None),
        ("POST", "stop", None),
        ("POST", "save", None),
        (
            "POST",
            "set_zero",
            Some(r#"{"confirm_advanced": true}"#),
        ),
    ] {
        let mut builder = Request::builder()
            .method(verb)
            .uri(format!("/api/motors/shoulder_actuator_a/{suffix}"));
        let req = if let Some(s) = body_json_str {
            builder = builder.header("content-type", "application/json");
            builder.body(Body::from(s)).unwrap()
        } else {
            builder.body(Body::empty()).unwrap()
        };
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

/// `GET /api/motors/:role/travel_limits` returns 404 + `no_travel_limits`
/// when no band has been configured yet. This is the SPA's signal to fall
/// back to "use spec defaults" instead of erroring loudly.
#[tokio::test]
async fn get_travel_limits_404s_when_unset() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "no_travel_limits");
}

/// PUT /api/motors/:role/travel_limits with a valid band roundtrips to the
/// matching GET. The lock-gate is satisfied implicitly: the request omits
/// `X-Rudy-Session` and `ensure_control("")` permits anonymous mutators
/// when the lock is free.
#[tokio::test]
async fn put_travel_limits_roundtrips_through_get() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({"min_rad": -1.0, "max_rad": 1.0})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let limits: TravelLimits = body_json(resp).await;
    assert!((limits.min_rad - -1.0).abs() < 1e-6);
    assert!((limits.max_rad - 1.0).abs() < 1e-6);
    assert!(limits.updated_at.is_some());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let limits: TravelLimits = body_json(resp).await;
    assert!((limits.max_rad - 1.0).abs() < 1e-6);
}

/// Bands wider than the hardware envelope are rejected before any disk
/// write happens.
#[tokio::test]
async fn put_travel_limits_rejects_out_of_range() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({"min_rad": -1000.0, "max_rad": 1000.0})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
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

/// Inverted bands are rejected too (and audited as denied).
#[tokio::test]
async fn put_travel_limits_rejects_inverted_band() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({"min_rad": 0.5, "max_rad": -0.5})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
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

/// `GET /api/motors/:role/inventory` returns the typed scalars + free-form
/// `extra` map; the SPA renders it directly in the Inventory tab.
#[tokio::test]
async fn get_inventory_returns_motor_record() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/inventory")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = body_json(resp).await;
    assert_eq!(
        v.get("role").and_then(|v| v.as_str()),
        Some("shoulder_actuator_a")
    );
    assert_eq!(v.get("can_id").and_then(|v| v.as_u64()), Some(0x08));
}

/// `PUT /api/motors/:role/verified` flips the flag and the next GET
/// reflects it (cache hot-swap is exercised end-to-end here).
#[tokio::test]
async fn put_verified_flips_flag() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({"verified": false, "note": "test"})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/verified")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let m: MotorSummary = body_json(resp).await;
    assert!(!m.verified);
}

/// `POST /api/estop` returns OK in mock mode and counts every present motor.
#[tokio::test]
async fn estop_returns_ok_envelope() {
    let (state, _dir) = common::make_state();
    let total_present = state
        .inventory
        .read()
        .expect("inventory")
        .motors
        .iter()
        .filter(|m| m.present)
        .count();
    let app = rudydae::build_app(state);

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
    let app = rudydae::build_app(state);

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

// ============================================================================
// Boot-time travel-band gate (Layers 0-6) + canonical role rename.
// ============================================================================

use rudydae::boot_state::BootState;

/// Layer 5: enable refuses 409 `not_homed` when the motor is in band but
/// the operator hasn't run the slow-ramp homer this power-cycle.
#[tokio::test]
async fn enable_not_homed_is_forbidden() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "not_homed");
}

/// Layer 5: enable returns 409 `not_ready` when telemetry hasn't classified
/// the motor yet.
#[tokio::test]
async fn enable_unknown_state_is_forbidden() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "not_ready");
}

/// Layer 5: enable returns 409 `out_of_band` when classifier says
/// OutOfBand. The operator must move the joint into band manually.
#[tokio::test]
async fn enable_out_of_band_is_forbidden() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(
        &state,
        "shoulder_actuator_a",
        BootState::OutOfBand {
            mech_pos_rad: 1.5,
            min_rad: -1.0,
            max_rad: 1.0,
        },
    );
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_band");
}

/// Belt-and-suspenders: even if state is forced to Homed, a stale-cached
/// position outside the configured band still blocks enable via Check A.
#[tokio::test]
async fn enable_homed_but_drifted_outside_band_is_forbidden() {
    let (state, _dir) = common::make_state();

    // Configure a band on the motor.
    let _ = state
        .inventory
        .write()
        .expect("inventory")
        .motors
        .iter_mut()
        .find(|m| m.role == "shoulder_actuator_a")
        .map(|m| {
            m.travel_limits = Some(rudydae::inventory::TravelLimits {
                min_rad: -1.0,
                max_rad: 1.0,
                updated_at: None,
            });
        });

    // Latest cached position is OUTSIDE the band; state insists Homed.
    {
        let mut latest = state.latest.write().expect("latest");
        latest.insert(
            "shoulder_actuator_a".into(),
            MotorFeedback {
                t_ms: 1_700_000_000_000,
                role: "shoulder_actuator_a".into(),
                can_id: 0x08,
                mech_pos_rad: 1.5,
                mech_vel_rad_s: 0.0,
                torque_nm: 0.0,
                vbus_v: 48.0,
                temp_c: 30.0,
                fault_sta: 0,
                warn_sta: 0,
            },
        );
    }
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_band");
}

/// Layer 5: home transitions BootState to Homed, then enable returns 200.
#[tokio::test]
async fn home_succeeds_then_enable_succeeds() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);
    let app = rudydae::build_app(state.clone());

    // POST /home with an empty body (defaults to target=0).
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/home")
                .header("content-type", "application/json")
                .body(Body::from(b"{}".to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "home should succeed in mock mode when motor is InBand"
    );

    let bs = rudydae::boot_state::current(&state, "shoulder_actuator_a");
    assert!(matches!(bs, BootState::Homed));

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "enable after home");
}

/// Home refuses to start when motor is OutOfBand.
#[tokio::test]
async fn home_when_out_of_band_is_forbidden() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(
        &state,
        "shoulder_actuator_a",
        BootState::OutOfBand {
            mech_pos_rad: 1.5,
            min_rad: -1.0,
            max_rad: 1.0,
        },
    );
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/home")
                .header("content-type", "application/json")
                .body(Body::from(b"{}".to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_band");
}

/// Home target outside the configured band returns 409 `out_of_band`.
#[tokio::test]
async fn home_target_outside_band_is_forbidden() {
    let (state, _dir) = common::make_state();
    let _ = state
        .inventory
        .write()
        .expect("inventory")
        .motors
        .iter_mut()
        .find(|m| m.role == "shoulder_actuator_a")
        .map(|m| {
            m.travel_limits = Some(rudydae::inventory::TravelLimits {
                min_rad: -1.0,
                max_rad: 1.0,
                updated_at: None,
            });
        });
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/home")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"target_rad": 5.0})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_band");
}

/// Layer 2: jog projecting > boot_max_step_rad delta is refused while not
/// Homed even when the projected position is in-band.
#[tokio::test]
async fn jog_step_too_large_when_not_homed_is_forbidden() {
    let (state, _dir) = common::make_state();
    let _ = state
        .inventory
        .write()
        .expect("inventory")
        .motors
        .iter_mut()
        .find(|m| m.role == "shoulder_actuator_a")
        .map(|m| {
            m.travel_limits = Some(rudydae::inventory::TravelLimits {
                min_rad: -1.0,
                max_rad: 1.0,
                updated_at: None,
            });
        });
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);
    let app = rudydae::build_app(state);

    // Velocity 0.5 rad/s × 1 s = 0.5 rad delta — well above the 0.087 rad ceiling.
    let body = serde_json::to_vec(&json!({"vel_rad_s": 0.5, "ttl_ms": 1000})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/jog")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "step_too_large");
}

/// Sweep-safe CAN I/O (fail-closed): when `state.latest[role]` is missing
/// or older than `safety.max_feedback_age_ms`, jog must refuse with
/// `409 stale_telemetry` rather than silently bypassing the band check.
///
/// This is the primary regression test for the "Sweep travel limits"
/// safety hole where bus contention froze `state.latest` and every
/// subsequent jog approved every projected position forever.
#[tokio::test]
async fn jog_refuses_on_stale_feedback() {
    let (state, _dir) = common::make_state();
    let _ = state
        .inventory
        .write()
        .expect("inventory")
        .motors
        .iter_mut()
        .find(|m| m.role == "shoulder_actuator_a")
        .map(|m| {
            m.travel_limits = Some(TravelLimits {
                min_rad: -1.0,
                max_rad: 1.0,
                updated_at: None,
            });
        });
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);

    // Seed a deliberately stale row: 500 ms old > the 100 ms default.
    {
        let mut latest = state.latest.write().expect("latest");
        let now_ms = chrono::Utc::now().timestamp_millis();
        latest.insert(
            "shoulder_actuator_a".into(),
            MotorFeedback {
                t_ms: now_ms - 500,
                role: "shoulder_actuator_a".into(),
                can_id: 0x08,
                mech_pos_rad: 0.0,
                mech_vel_rad_s: 0.0,
                torque_nm: 0.0,
                vbus_v: 48.0,
                temp_c: 30.0,
                fault_sta: 0,
                warn_sta: 0,
            },
        );
    }
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({"vel_rad_s": 0.1, "ttl_ms": 200})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/jog")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "stale_telemetry");
    assert!(
        err.detail
            .as_deref()
            .map(|d| d.contains("ms old"))
            .unwrap_or(false),
        "stale_telemetry detail should include the age, got {:?}",
        err.detail,
    );
}

/// Companion to `jog_refuses_on_stale_feedback`: when `state.latest`
/// has no row at all for the role, the same 409 fires.
#[tokio::test]
async fn jog_refuses_with_no_feedback() {
    let (state, _dir) = common::make_state();
    let _ = state
        .inventory
        .write()
        .expect("inventory")
        .motors
        .iter_mut()
        .find(|m| m.role == "shoulder_actuator_a")
        .map(|m| {
            m.travel_limits = Some(TravelLimits {
                min_rad: -1.0,
                max_rad: 1.0,
                updated_at: None,
            });
        });
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    // Note: deliberately do NOT call seed_feedback.
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({"vel_rad_s": 0.1, "ttl_ms": 200})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/jog")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "stale_telemetry");
}

/// `set_zero` resets BootState to Unknown so the operator must re-home
/// before enable will work. The flag is required (see
/// `set_zero_without_confirm_advanced_returns_400`), so this test sends
/// the opt-in body to exercise the success path.
#[tokio::test]
async fn set_zero_resets_boot_state_to_unknown() {
    let (state, _dir) = common::make_state();
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    let app = rudydae::build_app(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/set_zero")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"confirm_advanced": true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bs = rudydae::boot_state::current(&state, "shoulder_actuator_a");
    assert!(matches!(bs, BootState::Unknown));
}

/// `set_zero` is RAM-only by design (it issues type-6 only; no type-22
/// SaveParams). The audit log must record this fact unambiguously so an
/// operator reviewing history after a "did this survive the reboot?"
/// question can grep for the marker. Specifically:
///
/// - the audit entry's `action` is `set_zero_advanced` (not plain
///   `set_zero`), so the act of opting in is the act being audited;
/// - `details.persisted` is `false`;
/// - `details.confirm_advanced` is `true`;
/// - the response body echoes `persisted: false` so the SPA can show a
///   distinct treatment without parsing free-form prose.
#[tokio::test]
async fn set_zero_audit_records_not_persisted() {
    let (state, dir) = common::make_state();
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    let app = rudydae::build_app(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/set_zero")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"confirm_advanced": true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = body_json(resp).await;
    assert_eq!(
        body.get("persisted"),
        Some(&serde_json::Value::Bool(false)),
        "set_zero response body must include persisted:false; got {body}",
    );

    // Read the audit JSONL the test fixture wrote to and find our entry.
    // The `audit::AuditLog::write` flushes after every entry so a single
    // synchronous read is race-free.
    let audit_path = dir.path().join("audit.jsonl");
    let raw = std::fs::read_to_string(&audit_path)
        .expect("audit log should exist after a successful set_zero");
    let last_set_zero = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|entry| {
            entry.get("action").and_then(|v| v.as_str()) == Some("set_zero_advanced")
        })
        .last()
        .expect("audit log should contain a set_zero_advanced entry");
    assert_eq!(
        last_set_zero
            .get("details")
            .and_then(|d| d.get("persisted")),
        Some(&serde_json::Value::Bool(false)),
        "audit entry must include details.persisted=false; got {last_set_zero}",
    );
    assert_eq!(
        last_set_zero
            .get("details")
            .and_then(|d| d.get("confirm_advanced")),
        Some(&serde_json::Value::Bool(true)),
        "audit entry must include details.confirm_advanced=true; got {last_set_zero}",
    );
    assert_eq!(
        last_set_zero.get("result").and_then(|v| v.as_str()),
        Some("ok"),
    );
    assert_eq!(
        last_set_zero.get("target").and_then(|v| v.as_str()),
        Some("shoulder_actuator_a"),
    );
}

/// The raw `set_zero` endpoint is gated behind `confirm_advanced: true`
/// so a misclick from the SPA — or a copy-pasted curl command — can't
/// silently shift a commissioned motor's frame. Three calling patterns
/// must all be rejected with the same `400 requires_confirmation`:
///
/// 1. completely missing body (the historical SPA call shape, before
///    Phase A.2 of the commissioned-zero plan);
/// 2. empty JSON object `{}`, where `confirm_advanced` defaults to
///    `false` via `#[serde(default)]`;
/// 3. explicit `{"confirm_advanced": false}`.
///
/// Critically, in all three cases the BootState must be UNCHANGED — a
/// rejected request must not have any side effects, including the
/// "reset to Unknown" that an accepted re-zero would trigger.
#[tokio::test]
async fn set_zero_without_confirm_advanced_returns_400() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state.clone());

    for (label, body) in [
        ("missing body", None),
        ("empty object", Some(r#"{}"#)),
        ("explicit false", Some(r#"{"confirm_advanced": false}"#)),
    ] {
        common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);

        let mut builder = Request::builder()
            .method(Method::POST)
            .uri("/api/motors/shoulder_actuator_a/set_zero");
        let req = if let Some(b) = body {
            builder = builder.header("content-type", "application/json");
            builder.body(Body::from(b)).unwrap()
        } else {
            builder.body(Body::empty()).unwrap()
        };

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "{label}: expected 400, got {}",
            resp.status()
        );
        let err: ApiError = body_json(resp).await;
        assert_eq!(err.error, "requires_confirmation", "{label}");
        let detail = err.detail.as_deref().unwrap_or("");
        assert!(
            detail.contains("confirm_advanced"),
            "{label}: detail must mention confirm_advanced; got {detail:?}"
        );
        assert!(
            detail.contains("/commission"),
            "{label}: detail must point operators at /commission; got {detail:?}"
        );

        // Side-effect check: the boot state must be untouched. An accepted
        // set_zero would have reset it to Unknown.
        let bs = rudydae::boot_state::current(&state, "shoulder_actuator_a");
        assert!(
            matches!(bs, BootState::Homed),
            "{label}: rejected set_zero must not mutate boot state; got {bs:?}"
        );
    }
}

/// Successful commission against a mock-CAN backend writes the readback
/// value into `inventory.yaml` (mock readback = 0.0 per the documented
/// `RealCanHandle` stub contract) and bumps `commissioned_at` to a
/// fresh ISO 8601 timestamp. The response body matches the typed
/// `CommissionResp { ok, role, offset_rad, commissioned_at }` shape.
///
/// Specifically pins:
///
/// - the wire response shape (`ok: true`, `role`, `offset_rad`,
///   `commissioned_at`);
/// - that the on-disk `inventory.yaml` is rewritten atomically and the
///   in-memory `state.inventory` matches what hit the disk;
/// - that `commissioned_at` is parseable as ISO 8601;
/// - that a `SafetyEvent::Commissioned` fires on the broadcast channel
///   (so the dashboard can refresh without polling).
#[tokio::test]
async fn commission_endpoint_writes_inventory() {
    let (state, dir) = common::make_state();
    let app = rudydae::build_app(state.clone());

    // Subscribe BEFORE the request to guarantee no missed safety event.
    let mut safety_rx = state.safety_event_tx.subscribe();

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/commission")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = body_json(resp).await;
    assert_eq!(body["ok"], serde_json::Value::Bool(true));
    assert_eq!(body["role"], serde_json::Value::String("shoulder_actuator_a".into()));
    // Mock-CAN readback is 0.0 by stub contract.
    assert_eq!(body["offset_rad"].as_f64(), Some(0.0));
    let commissioned_at = body["commissioned_at"].as_str().expect("commissioned_at must be a string");
    chrono::DateTime::parse_from_rfc3339(commissioned_at)
        .expect("commissioned_at must be ISO 8601 RFC 3339");

    // On-disk inventory.yaml must match the in-memory state.
    let inv_on_disk = rudydae::inventory::Inventory::load(dir.path().join("inventory.yaml"))
        .expect("re-load inventory");
    let m = inv_on_disk
        .by_role("shoulder_actuator_a")
        .expect("motor present in re-loaded inventory");
    assert_eq!(m.commissioned_zero_offset, Some(0.0_f32));
    assert_eq!(m.commissioned_at.as_deref(), Some(commissioned_at));

    // In-memory state must also reflect the write.
    let in_memory = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .by_role("shoulder_actuator_a")
        .cloned()
        .unwrap();
    assert_eq!(in_memory.commissioned_zero_offset, Some(0.0_f32));
    assert_eq!(in_memory.commissioned_at.as_deref(), Some(commissioned_at));

    // SafetyEvent::Commissioned must fire so the dashboard can refresh.
    let evt = tokio::time::timeout(std::time::Duration::from_millis(200), safety_rx.recv())
        .await
        .expect("safety event must fire within 200ms")
        .expect("safety_event_tx must not be closed");
    match evt {
        rudydae::types::SafetyEvent::Commissioned { role, offset_rad, .. } => {
            assert_eq!(role, "shoulder_actuator_a");
            assert_eq!(offset_rad, 0.0);
        }
        other => panic!("expected SafetyEvent::Commissioned, got {other:?}"),
    }
}

/// `commission` against an unknown role returns the commission-specific
/// error envelope (`error: "commission_failed"`, `detail` mentioning the
/// failing step, `readback_rad: null`) — NOT the generic ApiError shape.
/// Critically the on-disk inventory.yaml must NOT be touched.
#[tokio::test]
async fn commission_endpoint_unknown_role_leaves_inventory_clean() {
    let (state, dir) = common::make_state();
    let app = rudydae::build_app(state.clone());

    let inv_path = dir.path().join("inventory.yaml");
    let inv_before = std::fs::read_to_string(&inv_path).expect("read inventory");

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/no_such_role/commission")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let body: serde_json::Value = body_json(resp).await;
    assert_eq!(body["error"], serde_json::Value::String("commission_failed".into()));
    let detail = body["detail"].as_str().unwrap_or("");
    assert!(detail.starts_with("step 2"), "detail must name failing step; got {detail:?}");
    assert!(detail.contains("unknown_motor"), "detail must mention unknown_motor; got {detail:?}");
    assert!(body["readback_rad"].is_null(), "no readback was performed; got {body}");

    // Inventory file must be byte-identical to the pre-request snapshot.
    let inv_after = std::fs::read_to_string(&inv_path).expect("re-read inventory");
    assert_eq!(inv_before, inv_after, "rejected commission must not touch inventory.yaml");
}

/// `commission` against an absent motor (inventory.yaml has
/// `present: false`) returns 409 with the commission-specific envelope
/// and leaves the inventory file untouched. Mirrors the
/// `motor_absent` rejection that every other CAN-talking endpoint uses,
/// but routed through the commission failure shape.
#[tokio::test]
async fn commission_endpoint_motor_absent_rejected_cleanly() {
    let (state, dir) = common::make_state();
    // Mark shoulder_actuator_a as absent.
    {
        let mut inv = state.inventory.write().expect("inventory");
        let m = inv.motors.iter_mut()
            .find(|m| m.role == "shoulder_actuator_a")
            .unwrap();
        m.present = false;
    }
    let app = rudydae::build_app(state.clone());

    let inv_path = dir.path().join("inventory.yaml");
    let inv_before = std::fs::read_to_string(&inv_path).expect("read inventory");

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/commission")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = body_json(resp).await;
    assert_eq!(body["error"], serde_json::Value::String("commission_failed".into()));
    let detail = body["detail"].as_str().unwrap_or("");
    assert!(detail.starts_with("step 2"), "detail must name failing step; got {detail:?}");
    assert!(detail.contains("motor_absent"), "detail must mention motor_absent; got {detail:?}");
    assert!(body["readback_rad"].is_null());

    let inv_after = std::fs::read_to_string(&inv_path).expect("re-read inventory");
    assert_eq!(inv_before, inv_after, "rejected commission must not touch inventory.yaml");
}

/// `commission` records its outcome in the audit log, including the
/// readback value on success. Same JSONL log we exercised in the
/// set_zero audit test.
#[tokio::test]
async fn commission_endpoint_audit_logs_readback() {
    let (state, dir) = common::make_state();
    let app = rudydae::build_app(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/commission")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let raw = std::fs::read_to_string(dir.path().join("audit.jsonl"))
        .expect("audit log must exist");
    let last = raw
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|e| e.get("action").and_then(|v| v.as_str()) == Some("commission"))
        .last()
        .expect("audit log must contain a commission entry");
    assert_eq!(last["result"].as_str(), Some("ok"));
    assert_eq!(last["target"].as_str(), Some("shoulder_actuator_a"));
    assert_eq!(last["details"]["step"].as_str(), Some("ok"));
    assert_eq!(last["details"]["readback_rad"].as_f64(), Some(0.0));
}

/// `restore_offset` on mock-CAN writes nothing but clears `OffsetChanged`
/// after verifying the simulated readback matches `commissioned_zero_offset`.
#[tokio::test]
async fn restore_offset_mock_clears_offset_changed() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state.clone());

    {
        let mut inv = state.inventory.write().expect("inventory poisoned");
        let m = inv
            .motors
            .iter_mut()
            .find(|m| m.role == "shoulder_actuator_a")
            .expect("fixture motor");
        m.commissioned_zero_offset = Some(0.05);
    }

    common::set_boot_state(
        &state,
        "shoulder_actuator_a",
        BootState::OffsetChanged {
            stored_rad: 0.05,
            current_rad: 0.12,
        },
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/restore_offset")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = body_json(resp).await;
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["restored_rad"], json!(0.05));
    assert_eq!(body["readback_rad"], json!(0.05));

    let bs = rudydae::boot_state::current(&state, "shoulder_actuator_a");
    assert!(matches!(bs, BootState::Unknown));
}

/// `restore_offset` requires `BootState::OffsetChanged`; other states get
/// 409 `restore_failed`.
#[tokio::test]
async fn restore_offset_rejects_when_not_offset_changed() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state.clone());
    {
        let mut inv = state.inventory.write().expect("inventory poisoned");
        let m = inv
            .motors
            .iter_mut()
            .find(|m| m.role == "shoulder_actuator_a")
            .expect("fixture motor");
        m.commissioned_zero_offset = Some(0.05);
    }
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/restore_offset")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = body_json(resp).await;
    assert_eq!(body["error"], json!("restore_failed"));
    assert!(body["detail"].as_str().unwrap_or("").contains("wrong_boot_state"));
}

/// Rename of an enabled motor used to refuse with 409 `motor_active` and
/// force the operator to context-switch to the Controls tab to click Stop.
/// The daemon now does that round-trip itself: stop on the bus, perform
/// the rename, re-enable on the new role. The response surfaces the
/// transition via `auto_stopped` / `auto_reenabled` so the SPA can show
/// "torque was briefly dropped" instead of pretending nothing happened.
#[tokio::test]
async fn rename_active_motor_auto_stops_and_reenables() {
    let (state, _dir) = common::make_state();
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    state.mark_enabled("shoulder_actuator_a");
    let app = rudydae::build_app(state.clone());

    let body = serde_json::to_vec(&json!({"new_role": "left_arm.shoulder_pitch"})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/rename")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp_json: serde_json::Value = body_json(resp).await;
    assert_eq!(resp_json["ok"], json!(true));
    assert_eq!(resp_json["new_role"], json!("left_arm.shoulder_pitch"));
    assert_eq!(resp_json["auto_stopped"], json!(true));
    assert_eq!(resp_json["auto_reenabled"], json!(true));
    // The enabled bit followed the role across the rename: caller can
    // immediately issue further mutating calls against the new role and
    // see a consistent gate.
    assert!(state.is_enabled("left_arm.shoulder_pitch"));
    assert!(!state.is_enabled("shoulder_actuator_a"));
}

/// Renaming a stopped motor goes through the no-side-effect path: the
/// daemon doesn't auto-stop (nothing to stop) and doesn't auto-reenable
/// (nothing to restore). Both flags should be absent / false. Regression
/// test for the bug where the gate keyed off `BootState::Homed`, which
/// Stop does not clear, so operators got `motor_active` forever.
#[tokio::test]
async fn rename_stopped_motor_skips_auto_stop_cycle() {
    let (state, _dir) = common::make_state();
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    state.mark_enabled("shoulder_actuator_a");
    state.mark_stopped("shoulder_actuator_a");
    assert!(matches!(
        rudydae::boot_state::current(&state, "shoulder_actuator_a"),
        BootState::Homed
    ));
    assert!(!state.is_enabled("shoulder_actuator_a"));

    let app = rudydae::build_app(state.clone());
    let body = serde_json::to_vec(&json!({"new_role": "left_arm.shoulder_pitch"})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/rename")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp_json: serde_json::Value = body_json(resp).await;
    // skip_serializing_if drops the false flags entirely; assert by
    // checking the field is absent OR explicitly false.
    let auto_stopped = resp_json
        .get("auto_stopped")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let auto_reenabled = resp_json
        .get("auto_reenabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(!auto_stopped, "stopped motor should not trigger auto-stop");
    assert!(
        !auto_reenabled,
        "stopped motor should not trigger auto-reenable"
    );
    assert!(!state.is_enabled("left_arm.shoulder_pitch"));
}

/// First-time `assign` (motor has no limb / joint_kind on file yet) is a
/// pure labeling operation and must not be blocked by the
/// motion-safety gate. Without this exemption the operator would have to
/// reboot the daemon (or `set_zero`) to unstick the assignment any time
/// they had homed the motor in the same session.
#[tokio::test]
async fn assign_first_time_bypasses_motor_active_gate() {
    let (state, _dir) = common::make_state();
    // Worst case: motor is currently enabled. First-time assign should
    // STILL go through, because changing a role string doesn't move the
    // motor and the in-memory state migrates atomically below.
    state.mark_enabled("shoulder_actuator_a");

    let app = rudydae::build_app(state.clone());
    let body =
        serde_json::to_vec(&json!({"limb": "left_arm", "joint_kind": "shoulder_pitch"})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/assign")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    // The enabled bit must follow the role across the rename so the next
    // operator action (e.g. another rename) sees a consistent gate.
    assert!(state.is_enabled("left_arm.shoulder_pitch"));
    assert!(!state.is_enabled("shoulder_actuator_a"));
}

/// Re-assigning an already-assigned, currently-enabled motor goes through
/// the same auto-stop / auto-reenable path that `rename` does. (First-time
/// `assign` skips the cycle entirely — there's nothing to gate, see
/// `assign_first_time_bypasses_motor_active_gate`.)
#[tokio::test]
async fn assign_already_assigned_motor_auto_stops_and_reenables() {
    let (state, _dir) = common::make_state();
    common::set_boot_state(&state, "shoulder_actuator_b", BootState::Homed);
    let app = rudydae::build_app(state.clone());
    let body =
        serde_json::to_vec(&json!({"limb": "left_arm", "joint_kind": "shoulder_roll"})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_b/assign")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Now mark it enabled and re-assign. The daemon should auto-stop,
    // perform the rename, and auto-reenable under the new role.
    state.mark_enabled("left_arm.shoulder_roll");
    let body =
        serde_json::to_vec(&json!({"limb": "left_arm", "joint_kind": "shoulder_pitch"})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/left_arm.shoulder_roll/assign")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp_json: serde_json::Value = body_json(resp).await;
    assert_eq!(resp_json["new_role"], json!("left_arm.shoulder_pitch"));
    assert_eq!(resp_json["auto_stopped"], json!(true));
    assert_eq!(resp_json["auto_reenabled"], json!(true));
    assert!(state.is_enabled("left_arm.shoulder_pitch"));
    assert!(!state.is_enabled("left_arm.shoulder_roll"));
}

/// `POST /stop` clears the in-memory enabled bit so the rename gate
/// unsticks immediately on the next request from the same operator.
#[tokio::test]
async fn stop_endpoint_clears_enabled_bit() {
    let (state, _dir) = common::make_state();
    state.mark_enabled("shoulder_actuator_a");
    let app = rudydae::build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(!state.is_enabled("shoulder_actuator_a"));
}

/// Rename rejects malformed (non-canonical) target roles.
#[tokio::test]
async fn rename_invalid_role_format_is_rejected() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({"new_role": "Bad-Role"})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/rename")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "invalid_role");
}

/// `GET /api/motors/:role/motion` returns 204 when no motion is running.
/// The SPA's `api.motion.current` distinguishes 204 from 200 + JSON;
/// any change here breaks the actuator detail page on mount.
#[tokio::test]
async fn get_motion_returns_204_when_idle() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/motion")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

/// `POST /api/motors/:role/motion/sweep` without travel limits accepts
/// the request (the start preflight runs against vel=0 so the
/// path-violation check doesn't trip there) but the spawned controller
/// exits on its first tick. Pin both halves so we don't accidentally
/// flip to a "reject up front" model that would make the SPA's
/// "configure travel limits first" hint go unused.
#[tokio::test]
async fn motion_sweep_without_travel_limits_self_terminates() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);
    let mut status_rx = state.motion_status_tx.subscribe();
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({"speed_rad_s": 0.1})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/sweep")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: serde_json::Value = body_json(resp).await;
    let run_id = payload["run_id"].as_str().unwrap().to_string();

    // The controller exits within one tick with TravelLimitViolation.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut saw_violation = false;
    while !saw_violation {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("sweep without limits never produced a stopped frame");
        }
        let frame = match tokio::time::timeout(remaining, status_rx.recv()).await {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => panic!("status channel closed: {e}"),
            Err(_) => panic!("timed out waiting for stopped frame"),
        };
        if frame.run_id != run_id {
            continue;
        }
        if matches!(frame.state, rudydae::motion::intent::MotionState::Stopped) {
            assert_eq!(frame.reason.as_deref(), Some("travel_limit_violation"));
            saw_violation = true;
        }
    }
}

/// `POST /api/motors/:role/motion/sweep` happy path: starts a sweep and
/// returns `{ run_id, clamped_speed_rad_s }`. Issues an immediate
/// `motion/stop` to keep the test fixture quiet.
#[tokio::test]
async fn motion_sweep_starts_and_returns_run_id() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);
    {
        let mut inv = state.inventory.write().expect("inventory");
        let m = inv
            .motors
            .iter_mut()
            .find(|m| m.role == "shoulder_actuator_a")
            .unwrap();
        m.travel_limits = Some(TravelLimits {
            min_rad: -0.5,
            max_rad: 0.5,
            updated_at: None,
        });
    }
    let app = rudydae::build_app(state);

    let body = serde_json::to_vec(&json!({"speed_rad_s": 0.1})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/sweep")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: serde_json::Value = body_json(resp).await;
    let run_id = payload["run_id"].as_str().expect("run_id string");
    assert!(!run_id.is_empty());
    let clamped = payload["clamped_speed_rad_s"]
        .as_f64()
        .expect("clamped_speed_rad_s number");
    assert!((clamped - 0.1).abs() < 1e-6);

    // Cleanup: stop returns `{ stopped: bool }`.
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: serde_json::Value = body_json(resp).await;
    assert_eq!(payload["stopped"], json!(true));
}

/// `POST /api/motors/:role/motion/stop` is idempotent and returns
/// `{ stopped: false }` when nothing was running.
#[tokio::test]
async fn motion_stop_when_idle_returns_false() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: serde_json::Value = body_json(resp).await;
    assert_eq!(payload["stopped"], json!(false));
}

/// `POST /api/motors/:role/motion/sweep` clamps a speed beyond
/// `MAX_PATTERN_VEL_RAD_S` (2.0) silently. The SPA mirrors this constant,
/// so a regression here surfaces as the slider top end no longer
/// matching the actual cap. Note this is intentionally HIGHER than the
/// jog cap (0.5) — sweep self-reverses inside the travel band, so it's
/// safe at speeds the dead-man jog wouldn't be.
#[tokio::test]
async fn motion_sweep_clamps_excessive_speed() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);
    {
        let mut inv = state.inventory.write().expect("inventory");
        let m = inv
            .motors
            .iter_mut()
            .find(|m| m.role == "shoulder_actuator_a")
            .unwrap();
        m.travel_limits = Some(TravelLimits {
            min_rad: -0.5,
            max_rad: 0.5,
            updated_at: None,
        });
    }
    let app = rudydae::build_app(state);

    // Request 10 rad/s — well above the 2.0 sweep cap and the 3.0
    // firmware envelope. The handler must clamp to MAX_PATTERN_VEL_RAD_S.
    let body = serde_json::to_vec(&json!({"speed_rad_s": 10.0})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/sweep")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: serde_json::Value = body_json(resp).await;
    let clamped = payload["clamped_speed_rad_s"].as_f64().unwrap();
    assert!(
        clamped <= 2.0 + 1e-6,
        "expected clamped speed <= 2.0, got {clamped}"
    );
    // And clamping must actually have taken effect (i.e. the test isn't
    // just passing because the request was below the cap).
    assert!(
        clamped >= 2.0 - 1e-6,
        "expected clamp to saturate at 2.0, got {clamped}"
    );

    let _ = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
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
        "POST   /api/motors/:role/commission",
        "POST   /api/motors/:role/restore_offset",
        "GET    /api/motors/:role/travel_limits",
        "PUT    /api/motors/:role/travel_limits",
        "POST   /api/motors/:role/jog",
        "GET    /api/motors/:role/motion",
        "POST   /api/motors/:role/motion/sweep",
        "POST   /api/motors/:role/motion/wave",
        "POST   /api/motors/:role/motion/jog",
        "POST   /api/motors/:role/motion/stop",
        "POST   /api/motors/:role/home",
        "POST   /api/motors/:role/rename",
        "POST   /api/motors/:role/assign",
        "POST   /api/home_all",
        "POST   /api/motors/:role/tests/:name",
        "GET    /api/motors/:role/inventory",
        "PUT    /api/motors/:role/verified",
        "POST   /api/estop",
        "GET    /api/reminders",
        "POST   /api/reminders",
        "PUT    /api/reminders/:id",
        "DELETE /api/reminders/:id",
    ];
}
