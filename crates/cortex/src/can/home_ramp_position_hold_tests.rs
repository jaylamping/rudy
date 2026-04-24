//! Pins the post-home hold contract: MIT spring-damper hold (not PP) with the
//! `[safety].hold_kp/kd_*` defaults, and a verification cap of
//! `2 × target_tolerance_rad` using **fresh** `state.latest` telemetry only.
//!
//! End-to-end stub coverage lives in `home_ramp_real_can_stub_tests` and
//! `tests/boot_orchestrator_lifecycle.rs` (`is_position_hold` after auto-home).

use std::collections::BTreeMap;

use chrono::Utc;

use super::finish_home_success;
use crate::audit::AuditLog;
use crate::config::{
    CanConfig, Config, HttpConfig, LogsConfig, PathsConfig, SafetyConfig, TelemetryConfig,
    WebTransportConfig,
};
use crate::inventory::Inventory;
use crate::reminders::ReminderStore;
use crate::spec;
use crate::state::AppState;
use crate::types::MotorFeedback;

fn fixture_state() -> (
    crate::state::SharedState,
    crate::inventory::Actuator,
    SafetyConfig,
) {
    let dir = tempfile::tempdir().unwrap();
    let spec_path = dir.path().join("robstride_rs03.yaml");
    std::fs::write(
        &spec_path,
        "schema_version: 2\nactuator_model: RS03\nfirmware_limits: {}\nobservables: {}\n",
    )
    .unwrap();
    let inv_path = dir.path().join("inv.yaml");
    std::fs::write(
        &inv_path,
        "schema_version: 2\ndevices:\n  - kind: actuator\n    role: m\n    can_bus: can0\n    can_id: 1\n    present: true\n    family:\n      kind: robstride\n      model: rs03\n    travel_limits:\n      min_rad: -1.0\n      max_rad: 1.0\n",
    )
    .unwrap();
    let tol = crate::config::default_target_tolerance_rad();
    let cfg = Config {
        http: HttpConfig {
            bind: "127.0.0.1:0".into(),
        },
        webtransport: WebTransportConfig {
            bind: "127.0.0.1:0".into(),
            enabled: false,
            cert_path: None,
            key_path: None,
        },
        paths: PathsConfig {
            actuator_spec: spec_path.clone(),
            inventory: inv_path.clone(),
            inventory_seed: None,
            audit_log: dir.path().join("audit.jsonl"),
        },
        can: CanConfig {
            mock: true,
            buses: vec![],
        },
        telemetry: TelemetryConfig {
            poll_interval_ms: 10,
        },
        safety: SafetyConfig {
            require_verified: false,
            boot_max_step_rad: 0.087,
            step_size_rad: 0.02,
            tick_interval_ms: 5,
            homing_speed_rad_s: None,
            tracking_error_max_rad: 0.05,
            tracking_error_grace_ticks: 0,
            // Must exceed the 500 ms post-hold settle in `finish_home_success`, otherwise
            // a single snapshot taken at test start goes stale during `sleep` and every
            // verification incorrectly trips `hold_verification_stale_telemetry`.
            tracking_freshness_max_age_ms: 600,
            tracking_error_debounce_ticks: 3,
            band_violation_debounce_ticks: 3,
            boot_tracking_error_max_rad: 0.05,
            target_tolerance_rad: tol,
            target_dwell_ticks: 1,
            homer_timeout_ms: 5_000,
            max_feedback_age_ms: 100,
            commission_readback_tolerance_rad: 1e-3,
            auto_home_on_boot: true,
            scan_on_boot: true,
            hold_kp_nm_per_rad: 10.0,
            hold_kd_nm_s_per_rad: 0.5,
        },
        logs: LogsConfig {
            db_path: dir.path().join("logs.db"),
            ..LogsConfig::default()
        },
    };
    let specs = spec::load_robstride_specs(dir.path(), Some(&spec_path)).unwrap();
    let inv = Inventory::load(&inv_path).unwrap();
    let motor = inv.actuators().next().cloned().expect("fixture actuator");
    let audit = AuditLog::open(dir.path().join("audit.jsonl")).unwrap();
    let reminders = ReminderStore::open(dir.path().join("reminders.json")).unwrap();
    let safety = cfg.safety.clone();
    let state = std::sync::Arc::new(AppState::new(cfg, specs, inv, audit, None, reminders));
    (state, motor, safety)
}

fn seed_latest(state: &crate::state::SharedState, role: &str, mech_pos_rad: f32, t_ms: i64) {
    let mut w = state.latest.write().expect("latest poisoned");
    w.insert(
        role.to_string(),
        MotorFeedback {
            t_ms,
            role: role.to_string(),
            can_id: 1,
            mech_pos_rad,
            mech_vel_rad_s: 0.0,
            torque_nm: 0.0,
            vbus_v: 48.0,
            temp_c: 30.0,
            fault_sta: 0,
            warn_sta: 0,
        },
    );
}

#[tokio::test]
async fn hold_verification_passes_when_mech_pos_inside_2x_tolerance() {
    let (state, motor, safety) = fixture_state();
    let role = motor.common.role.clone();
    let target = 0.0_f32;
    let tol = safety.target_tolerance_rad;
    let now = Utc::now().timestamp_millis();
    seed_latest(&state, &role, target + 1.5 * tol, now);

    let r = finish_home_success(&state, &motor, &role, target, &safety, 0.0, tol).await;
    assert!(r.is_ok(), "expected Ok, got {r:?}");
    assert!(
        state.is_position_hold(&role),
        "success path should leave position_hold set"
    );
}

#[tokio::test]
async fn hold_verification_fails_when_mech_pos_droops_past_2x_tolerance() {
    let (state, motor, safety) = fixture_state();
    let role = motor.common.role.clone();
    let target = 0.0_f32;
    let tol = safety.target_tolerance_rad;
    let droop = target + 3.0 * tol;
    let now = Utc::now().timestamp_millis();
    seed_latest(&state, &role, droop, now);

    let r = finish_home_success(&state, &motor, &role, target, &safety, 0.0, tol).await;
    assert_eq!(
        r,
        Err(("hold_verification_failed".into(), droop)),
        "expected hold_verification_failed with reported mech_pos"
    );
    assert!(
        !state.is_position_hold(&role),
        "fail path should clear position_hold via mark_stopped"
    );
}

#[tokio::test]
async fn hold_verification_fails_when_telemetry_stale() {
    let (state, motor, safety) = fixture_state();
    let role = motor.common.role.clone();
    let target = 0.0_f32;
    // Older than `tracking_freshness_max_age_ms` even after the 500 ms settle sleep.
    let stale_ms = Utc::now().timestamp_millis() - 2_000;
    seed_latest(&state, &role, 0.0, stale_ms);

    let r = finish_home_success(
        &state,
        &motor,
        &role,
        target,
        &safety,
        0.0,
        safety.target_tolerance_rad,
    )
    .await;
    match r {
        Err((reason, pos)) => {
            assert_eq!(reason, "hold_verification_stale_telemetry");
            assert!((pos - 0.0).abs() < 1e-5);
        }
        other => panic!("expected stale Err, got {other:?}"),
    }
    assert!(!state.is_position_hold(&role));
}

#[tokio::test]
async fn hold_verification_fails_when_telemetry_missing() {
    let (state, motor, safety) = fixture_state();
    let role = motor.common.role.clone();
    let target = 0.0_f32;
    *state.latest.write().expect("latest poisoned") = BTreeMap::new();

    let last = 0.05_f32;
    let r = finish_home_success(
        &state,
        &motor,
        &role,
        target,
        &safety,
        last,
        safety.target_tolerance_rad,
    )
    .await;
    assert_eq!(
        r,
        Err(("hold_verification_stale_telemetry".into(), last)),
        "missing row should return last_measured from homer exit"
    );
    assert!(!state.is_position_hold(&role));
}

#[test]
fn mit_hold_defaults_are_conservative_spring() {
    // Sanity-check the `[safety]` defaults wired into `finish_home_success`.
    // Spring stiffness must be:
    //   - high enough to resist gravity droop on the loaded shoulder/elbow
    //     joints during the 500 ms post-home verification window
    //     (empirically kp ~= 100-150 on shoulder_pitch with arm payload —
    //     see the 2026-04-23 bumps 10→40→120 captured in
    //     `default_hold_kp_nm_per_rad`),
    //   - low enough that an operator can still push the joint by hand
    //     without the firmware fighting back hard enough to feel locked
    //     (~250-300 Nm/rad starts to feel notchy on the RS03 by hand;
    //     keep the global cap well below that — per-joint overrides on
    //     `ActuatorCommon::hold_kp_nm_per_rad` carry the heavy joints).
    // Damping ratio should track kp via sqrt; if kp moves a lot, kd needs
    // to follow or the spring will ring.
    let kp = crate::config::default_hold_kp_nm_per_rad();
    let kd = crate::config::default_hold_kd_nm_s_per_rad();
    assert!(
        (40.0..=200.0).contains(&kp),
        "kp out of sane range for an RS03 hand-pushable spring: {kp}"
    );
    assert!(
        (0.5..=4.0).contains(&kd),
        "kd out of sane range relative to kp: {kd}"
    );
}
