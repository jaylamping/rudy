//! Tests for `enforce_position_with_path` use a real `AppState` so the
//! TravelLimits lookup goes through the same code path the production
//! daemon does. Helper lives in the integration `tests/common`; here
//! we duplicate a tiny subset to keep the unit test hermetic.

use super::*;
use crate::audit::AuditLog;
use crate::can;
use crate::can::angle::UnwrappedAngle;
use crate::config::{
    CanConfig, Config, HttpConfig, LogsConfig, MotionBackend, PathsConfig, RuntimeDbConfig,
    SafetyConfig, TelemetryConfig, WebTransportConfig,
};
use crate::inventory::Inventory;
use crate::reminders::ReminderStore;
use crate::spec;
use crate::state::AppState;
use std::sync::Arc;

fn state_with_band(min: f32, max: f32) -> (crate::state::SharedState, tempfile::TempDir) {
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
        format!(
            "schema_version: 2\ndevices:\n  - kind: actuator\n    role: m\n    can_bus: can0\n    can_id: 1\n    present: true\n    family:\n      kind: robstride\n      model: rs03\n    travel_limits:\n      min_rad: {min}\n      max_rad: {max}\n"
        ),
    )
    .unwrap();
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
            tracking_freshness_max_age_ms: 100,
            tracking_error_debounce_ticks: 3,
            fatal_warn_mask: 0x1,
            band_violation_debounce_ticks: 3,
            boot_tracking_error_max_rad: 0.05,
            target_tolerance_rad: 0.005,
            target_dwell_ticks: 5,
            // Stub tests don't model velocity-loop dynamics; disable the
            // gate so existing position-only dwell assertions still hold.
            target_dwell_max_vel_rad_s: f32::INFINITY,
            homer_timeout_ms: 5_000,
            max_feedback_age_ms: 100,
            commission_readback_tolerance_rad: 1e-3,
            auto_home_on_boot: true,
            scan_on_boot: true,
            hold_kp_nm_per_rad: 10.0,
            hold_kd_nm_s_per_rad: 0.5,
            motion_backend: MotionBackend::Velocity,
            mit_command_rate_hz: 100.0,
            mit_max_angle_step_rad: 0.087,
            mit_lpf_cutoff_hz: 6.0,
            mit_min_jerk_blend_ms: 0.0,
        },
        logs: LogsConfig {
            db_path: dir.path().join("logs.db"),
            ..LogsConfig::default()
        },
        runtime: RuntimeDbConfig::default(),
    };
    let specs = spec::load_robstride_specs(dir.path(), Some(&spec_path)).unwrap();
    let inv = Inventory::load(&inv_path).unwrap();
    let audit = AuditLog::open(dir.path().join("audit.jsonl")).unwrap();
    let real_can = can::build_handle(&cfg, &inv).unwrap();
    let reminders = ReminderStore::open(dir.path().join("reminders.json")).unwrap();
    (
        Arc::new(AppState::new(cfg, specs, inv, audit, real_can, reminders)),
        dir,
    )
}

#[test]
fn path_check_in_band_returns_inband_with_delta() {
    let (s, _d) = state_with_band(-1.0, 1.0);
    let r = enforce_position_with_path(&s, "m", UnwrappedAngle::new(0.0), UnwrappedAngle::new(0.5))
        .unwrap();
    match r {
        BandCheck::InBand { delta_rad, .. } => {
            assert!((delta_rad - 0.5).abs() < 1e-5);
        }
        other => panic!("expected InBand, got {other:?}"),
    }
}

#[test]
fn path_check_target_outside_band_returns_outofband() {
    let (s, _d) = state_with_band(-1.0, 1.0);
    let r = enforce_position_with_path(&s, "m", UnwrappedAngle::new(0.0), UnwrappedAngle::new(1.5))
        .unwrap();
    assert!(matches!(r, BandCheck::OutOfBand { .. }));
}

#[test]
fn path_check_current_outside_band_returns_pathviolation() {
    let (s, _d) = state_with_band(-1.0, 1.0);
    let r = enforce_position_with_path(&s, "m", UnwrappedAngle::new(1.5), UnwrappedAngle::new(0.0))
        .unwrap();
    assert!(matches!(r, BandCheck::PathViolation { .. }));
}

#[test]
fn path_check_no_band_returns_nolimit() {
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
        "schema_version: 2\ndevices:\n  - kind: actuator\n    role: m\n    can_bus: can0\n    can_id: 1\n    present: true\n    family:\n      kind: robstride\n      model: rs03\n",
    )
    .unwrap();
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
            tracking_freshness_max_age_ms: 100,
            tracking_error_debounce_ticks: 3,
            fatal_warn_mask: 0x1,
            band_violation_debounce_ticks: 3,
            boot_tracking_error_max_rad: 0.05,
            target_tolerance_rad: 0.005,
            target_dwell_ticks: 5,
            // Stub tests don't model velocity-loop dynamics; disable the
            // gate so existing position-only dwell assertions still hold.
            target_dwell_max_vel_rad_s: f32::INFINITY,
            homer_timeout_ms: 5_000,
            max_feedback_age_ms: 100,
            commission_readback_tolerance_rad: 1e-3,
            auto_home_on_boot: true,
            scan_on_boot: true,
            hold_kp_nm_per_rad: 10.0,
            hold_kd_nm_s_per_rad: 0.5,
            motion_backend: MotionBackend::Velocity,
            mit_command_rate_hz: 100.0,
            mit_max_angle_step_rad: 0.087,
            mit_lpf_cutoff_hz: 6.0,
            mit_min_jerk_blend_ms: 0.0,
        },
        logs: LogsConfig {
            db_path: dir.path().join("logs.db"),
            ..LogsConfig::default()
        },
        runtime: RuntimeDbConfig::default(),
    };
    let specs = spec::load_robstride_specs(dir.path(), Some(&spec_path)).unwrap();
    let inv = Inventory::load(&inv_path).unwrap();
    let audit = AuditLog::open(dir.path().join("audit.jsonl")).unwrap();
    let real_can = can::build_handle(&cfg, &inv).unwrap();
    let reminders = ReminderStore::open(dir.path().join("reminders.json")).unwrap();
    let s = Arc::new(AppState::new(cfg, specs, inv, audit, real_can, reminders));
    let r = enforce_position_with_path(
        &s,
        "m",
        UnwrappedAngle::new(100.0),
        UnwrappedAngle::new(100.0),
    )
    .unwrap();
    assert!(matches!(r, BandCheck::NoLimit));
}
