//! Enable/stop/save, set_zero, commission, restore_offset, rename, assign, stop.
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

use cortex::boot_state::BootState;

#[tokio::test]
async fn control_endpoints_return_ok_envelope_for_verified_motor() {
    let (state, _dir) = common::make_state();
    // Pre-date the boot-time gate: this test only cares about the envelope
    // shape, not the gate. Force every motor to Homed so enable doesn't
    // trip the new ritual checks.
    common::seed_feedback(&state);
    common::force_homed(&state);
    let app = cortex::build_app(state);

    // (verb, suffix, body) triples. `set_zero` is the only one that
    // requires a body (the `confirm_advanced: true` opt-in flag);
    // every other control endpoint is bodyless.
    for (verb, suffix, body_json_str) in [
        ("POST", "enable", None),
        ("POST", "stop", None),
        ("POST", "save", None),
        ("POST", "set_zero", Some(r#"{"confirm_advanced": true}"#)),
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
    let app = cortex::build_app(state);

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

// ============================================================================
// Boot-time travel-band gate (Layers 0-6) + canonical role rename.
// ============================================================================

/// Layer 5: enable refuses 409 `not_homed` when the motor is in band but
/// the operator hasn't run the home-ramp homer this power-cycle.
#[tokio::test]
async fn enable_not_homed_is_forbidden() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);
    let app = cortex::build_app(state);

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
    let app = cortex::build_app(state);

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
    let app = cortex::build_app(state);

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

/// Belt-and-suspenders: even if state is forced to Homed, a fresh cached
/// position outside the configured band still blocks enable via Check A.
#[tokio::test]
async fn enable_homed_but_drifted_outside_band_is_forbidden() {
    let (state, _dir) = common::make_state();

    // Configure a band on the motor.
    {
        let mut inv = state.inventory.write().expect("inventory");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").unwrap();
        a.common.travel_limits = Some(cortex::inventory::TravelLimits {
            min_rad: -1.0,
            max_rad: 1.0,
            updated_at: None,
        });
    }

    // Latest cached position is OUTSIDE the band; state insists Homed.
    {
        let mut latest = state.latest.write().expect("latest");
        latest.insert(
            "shoulder_actuator_a".into(),
            MotorFeedback {
                t_ms: chrono::Utc::now().timestamp_millis(),
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
    let app = cortex::build_app(state);

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

#[tokio::test]
async fn enable_homed_but_stale_feedback_is_forbidden() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    {
        let mut latest = state.latest.write().expect("latest");
        let fb = latest.get_mut("shoulder_actuator_a").expect("seeded");
        fb.t_ms = chrono::Utc::now().timestamp_millis() - 10_000;
    }
    let app = cortex::build_app(state);

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
    assert_eq!(err.error, "stale_telemetry");
}

#[tokio::test]
async fn enable_homed_but_missing_feedback_is_forbidden() {
    let (state, _dir) = common::make_state();
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    state
        .latest
        .write()
        .expect("latest")
        .remove("shoulder_actuator_a");
    let app = cortex::build_app(state);

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
    let app = cortex::build_app(state.clone());

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

    let bs = cortex::boot_state::current(&state, "shoulder_actuator_a");
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
    let app = cortex::build_app(state.clone());

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
        .rfind(|entry| entry.get("action").and_then(|v| v.as_str()) == Some("set_zero_advanced"))
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
/// so a misclick from the SPA ΓÇö or a copy-pasted curl command ΓÇö can't
/// silently shift a commissioned motor's frame. Three calling patterns
/// must all be rejected with the same `400 requires_confirmation`:
///
/// 1. completely missing body (the historical SPA call shape, before
///    Phase A.2 of the commissioned-zero plan);
/// 2. empty JSON object `{}`, where `confirm_advanced` defaults to
///    `false` via `#[serde(default)]`;
/// 3. explicit `{"confirm_advanced": false}`.
///
/// Critically, in all three cases the BootState must be UNCHANGED ΓÇö a
/// rejected request must not have any side effects, including the
/// "reset to Unknown" that an accepted re-zero would trigger.
#[tokio::test]
async fn set_zero_without_confirm_advanced_returns_400() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state.clone());

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
        let bs = cortex::boot_state::current(&state, "shoulder_actuator_a");
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
    let app = cortex::build_app(state.clone());

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
    assert_eq!(
        body["role"],
        serde_json::Value::String("shoulder_actuator_a".into())
    );
    // Mock-CAN readback is 0.0 by stub contract.
    assert_eq!(body["offset_rad"].as_f64(), Some(0.0));
    let commissioned_at = body["commissioned_at"]
        .as_str()
        .expect("commissioned_at must be a string");
    chrono::DateTime::parse_from_rfc3339(commissioned_at)
        .expect("commissioned_at must be ISO 8601 RFC 3339");

    // On-disk inventory.yaml must match the in-memory state.
    let inv_on_disk = cortex::inventory::Inventory::load(dir.path().join("inventory.yaml"))
        .expect("re-load inventory");
    let m = inv_on_disk
        .actuator_by_role("shoulder_actuator_a")
        .expect("motor present in re-loaded inventory");
    assert_eq!(m.common.commissioned_zero_offset, Some(0.0_f32));
    assert_eq!(m.common.commissioned_at.as_deref(), Some(commissioned_at));

    // In-memory state must also reflect the write.
    let in_memory = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role("shoulder_actuator_a")
        .cloned()
        .unwrap();
    assert_eq!(in_memory.common.commissioned_zero_offset, Some(0.0_f32));
    assert_eq!(
        in_memory.common.commissioned_at.as_deref(),
        Some(commissioned_at)
    );

    // SafetyEvent::Commissioned must fire so the dashboard can refresh.
    let evt = tokio::time::timeout(std::time::Duration::from_millis(200), safety_rx.recv())
        .await
        .expect("safety event must fire within 200ms")
        .expect("safety_event_tx must not be closed");
    match evt {
        cortex::types::SafetyEvent::Commissioned {
            role, offset_rad, ..
        } => {
            assert_eq!(role, "shoulder_actuator_a");
            assert_eq!(offset_rad, 0.0);
        }
        other => panic!("expected SafetyEvent::Commissioned, got {other:?}"),
    }
}

/// Non-Linux: `AppState` holds `Some(RealCanHandle)` so `POST /commission`
/// exercises the CAN branch; the dev-host stub fails `set_zero` immediately.
/// Inventory must not be rewritten (`write_atomic` never runs).
#[cfg(not(target_os = "linux"))]
#[tokio::test]
async fn commission_endpoint_can_failure_leaves_inventory_clean() {
    let (state, dir) = common::make_state_commission_can_path_fails();
    let app = cortex::build_app(state.clone());

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
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);

    let body: serde_json::Value = body_json(resp).await;
    assert_eq!(
        body["error"],
        serde_json::Value::String("commission_failed".into())
    );
    assert!(
        body["detail"]
            .as_str()
            .unwrap_or("")
            .contains("step 3 (set_zero)"),
        "detail={:?}",
        body["detail"]
    );
    assert!(body["readback_rad"].is_null());

    let inv_after = std::fs::read_to_string(&inv_path).expect("read inventory");
    assert_eq!(inv_before, inv_after);

    let m = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role("shoulder_actuator_a")
        .cloned()
        .unwrap();
    assert_eq!(m.common.commissioned_zero_offset, None);
    assert_eq!(m.common.commissioned_at, None);
}

/// `commission` against an unknown role returns the commission-specific
/// error envelope (`error: "commission_failed"`, `detail` mentioning the
/// failing step, `readback_rad: null`) ΓÇö NOT the generic ApiError shape.
/// Critically the on-disk inventory.yaml must NOT be touched.
#[tokio::test]
async fn commission_endpoint_unknown_role_leaves_inventory_clean() {
    let (state, dir) = common::make_state();
    let app = cortex::build_app(state.clone());

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
    assert_eq!(
        body["error"],
        serde_json::Value::String("commission_failed".into())
    );
    let detail = body["detail"].as_str().unwrap_or("");
    assert!(
        detail.starts_with("step 2"),
        "detail must name failing step; got {detail:?}"
    );
    assert!(
        detail.contains("unknown_motor"),
        "detail must mention unknown_motor; got {detail:?}"
    );
    assert!(
        body["readback_rad"].is_null(),
        "no readback was performed; got {body}"
    );

    // Inventory file must be byte-identical to the pre-request snapshot.
    let inv_after = std::fs::read_to_string(&inv_path).expect("re-read inventory");
    assert_eq!(
        inv_before, inv_after,
        "rejected commission must not touch inventory.yaml"
    );
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
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").unwrap();
        a.common.present = false;
    }
    let app = cortex::build_app(state.clone());

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
    assert_eq!(
        body["error"],
        serde_json::Value::String("commission_failed".into())
    );
    let detail = body["detail"].as_str().unwrap_or("");
    assert!(
        detail.starts_with("step 2"),
        "detail must name failing step; got {detail:?}"
    );
    assert!(
        detail.contains("motor_absent"),
        "detail must mention motor_absent; got {detail:?}"
    );
    assert!(body["readback_rad"].is_null());

    let inv_after = std::fs::read_to_string(&inv_path).expect("re-read inventory");
    assert_eq!(
        inv_before, inv_after,
        "rejected commission must not touch inventory.yaml"
    );
}

/// `commission` records its outcome in the audit log, including the
/// readback value on success. Same JSONL log we exercised in the
/// set_zero audit test.
#[tokio::test]
async fn commission_endpoint_audit_logs_readback() {
    let (state, dir) = common::make_state();
    let app = cortex::build_app(state.clone());

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

    let raw =
        std::fs::read_to_string(dir.path().join("audit.jsonl")).expect("audit log must exist");
    let last = raw
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .rfind(|e| e.get("action").and_then(|v| v.as_str()) == Some("commission"))
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
    let app = cortex::build_app(state.clone());

    {
        let mut inv = state.inventory.write().expect("inventory poisoned");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").expect("fixture motor");
        a.common.commissioned_zero_offset = Some(0.05);
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

    let bs = cortex::boot_state::current(&state, "shoulder_actuator_a");
    assert!(matches!(bs, BootState::Unknown));
}

/// `restore_offset` requires `BootState::OffsetChanged`; other states get
/// 409 `restore_failed`.
#[tokio::test]
async fn restore_offset_rejects_when_not_offset_changed() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state.clone());
    {
        let mut inv = state.inventory.write().expect("inventory poisoned");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").expect("fixture motor");
        a.common.commissioned_zero_offset = Some(0.05);
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
    assert!(body["detail"]
        .as_str()
        .unwrap_or("")
        .contains("wrong_boot_state"));
}

/// Successful `commission` on one motor must not write commissioning fields
/// on sibling inventory rows.
#[tokio::test]
async fn commission_leaves_sibling_motor_uncommissioned() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state.clone());

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

    let b = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role("shoulder_actuator_b")
        .cloned()
        .expect("fixture motor b");
    assert_eq!(b.common.commissioned_zero_offset, None);
    assert_eq!(b.common.commissioned_at, None);
}

/// `restore_offset` records a successful outcome in the audit log with the
/// restored and readback radians (mock-CAN readback equals inventory).
#[tokio::test]
async fn restore_offset_audit_logs_success() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state.clone());
    let audit_path = state.cfg.paths.audit_log.clone();

    {
        let mut inv = state.inventory.write().expect("inventory poisoned");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").expect("fixture motor");
        a.common.commissioned_zero_offset = Some(0.05);
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

    let raw = std::fs::read_to_string(&audit_path).expect("read audit log");
    let last = raw
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .rfind(|e| e.get("action").and_then(|v| v.as_str()) == Some("restore_offset"))
        .expect("audit log must contain restore_offset");
    assert_eq!(last["result"].as_str(), Some("ok"));
    assert_eq!(last["target"].as_str(), Some("shoulder_actuator_a"));
    assert_eq!(last["details"]["step"].as_str(), Some("ok"));
    let restored = last["details"]["restored_rad"]
        .as_f64()
        .expect("restored_rad");
    let readback = last["details"]["readback_rad"]
        .as_f64()
        .expect("readback_rad");
    assert!((restored - 0.05).abs() < 1e-5, "restored_rad: {restored}");
    assert!((readback - 0.05).abs() < 1e-5, "readback_rad: {readback}");
}

/// After `restore_offset`, the boot orchestrator idempotency flag is cleared
/// so a later qualifying telemetry transition can auto-home again.
#[tokio::test]
async fn restore_offset_clears_boot_orchestrator_attempted() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state.clone());

    {
        let mut inv = state.inventory.write().expect("inventory poisoned");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").expect("fixture motor");
        a.common.commissioned_zero_offset = Some(0.05);
    }

    common::set_boot_state(
        &state,
        "shoulder_actuator_a",
        BootState::OffsetChanged {
            stored_rad: 0.05,
            current_rad: 0.12,
        },
    );

    state
        .boot_orchestrator_attempted
        .lock()
        .expect("poisoned")
        .insert("shoulder_actuator_a".into());

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

    assert!(!state
        .boot_orchestrator_attempted
        .lock()
        .expect("poisoned")
        .contains("shoulder_actuator_a"));
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
    let app = cortex::build_app(state.clone());

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
        cortex::boot_state::current(&state, "shoulder_actuator_a"),
        BootState::Homed
    ));
    assert!(!state.is_enabled("shoulder_actuator_a"));

    let app = cortex::build_app(state.clone());
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

    let app = cortex::build_app(state.clone());
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
/// `assign` skips the cycle entirely ΓÇö there's nothing to gate, see
/// `assign_first_time_bypasses_motor_active_gate`.)
#[tokio::test]
async fn assign_already_assigned_motor_auto_stops_and_reenables() {
    let (state, _dir) = common::make_state();
    common::set_boot_state(&state, "shoulder_actuator_b", BootState::Homed);
    let app = cortex::build_app(state.clone());
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
    let app = cortex::build_app(state.clone());
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
    let app = cortex::build_app(state);

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
