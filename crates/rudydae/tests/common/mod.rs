//! Shared test fixtures.
//!
//! Boots rudydae's `AppState` against in-memory minimal YAML so tests don't
//! depend on the prod inventory (which intentionally has no `verified: true`
//! motors and grows over time).

#![allow(dead_code)] // helpers are imported per-test, some only by some tests

use std::io::Write;
use std::sync::Arc;

use rudydae::audit::AuditLog;
use rudydae::can;
use rudydae::config::{
    CanConfig, Config, HttpConfig, PathsConfig, SafetyConfig, TelemetryConfig, WebTransportConfig,
};
use rudydae::inventory::Inventory;
use rudydae::reminders::ReminderStore;
use rudydae::spec::ActuatorSpec;
use rudydae::state::{AppState, SharedState};

const SPEC_YAML: &str = r#"
schema_version: 2
actuator_model: TEST_RS03

firmware_limits:
  limit_torque:
    index: 0x700B
    type: float
    units: nm
    hardware_range: [0.0, 60.0]
  limit_spd:
    index: 0x7017
    type: float
    units: rad_per_s
    hardware_range: [0.0, 20.0]
  run_mode:
    index: 0x7005
    type: uint8

observables:
  mech_pos:
    index: 0x3016
    type: float
    units: rad
  vbus:
    index: 0x300C
    type: float
    units: volts
"#;

const INVENTORY_YAML: &str = r#"
schema_version: 1
motors:
  - role: shoulder_actuator_a
    can_bus: can1
    can_id: 0x08
    firmware_version: "1.2.3"
    verified: true
  - role: shoulder_actuator_b
    can_bus: can1
    can_id: 0x09
    firmware_version: "1.2.3"
    verified: false
"#;

/// Build a temp-rooted SharedState with mock CAN, no TLS, no WT (so we don't
/// need cert files). The audit log goes to the per-test `tempdir`.
pub fn make_state() -> (SharedState, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");

    let spec_path = dir.path().join("spec.yaml");
    std::fs::write(&spec_path, SPEC_YAML).unwrap();

    let inv_path = dir.path().join("inventory.yaml");
    std::fs::write(&inv_path, INVENTORY_YAML).unwrap();

    let audit_path = dir.path().join("audit.jsonl");

    let cfg = Config {
        http: HttpConfig {
            bind: "127.0.0.1:0".into(),
        },
        webtransport: WebTransportConfig {
            // disabled in most tests so we don't need to load a cert
            bind: "127.0.0.1:0".into(),
            enabled: false,
            cert_path: None,
            key_path: None,
        },
        paths: PathsConfig {
            actuator_spec: spec_path.clone(),
            inventory: inv_path.clone(),
            audit_log: audit_path.clone(),
        },
        can: CanConfig {
            mock: true,
            buses: vec![],
        },
        telemetry: TelemetryConfig {
            poll_interval_ms: 10,
        },
        safety: SafetyConfig {
            require_verified: true,
        },
    };

    let spec = ActuatorSpec::load(&spec_path).expect("load spec");
    let inv = Inventory::load(&inv_path).expect("load inventory");
    let audit = AuditLog::open(&audit_path).expect("open audit");
    let real_can = can::build_handle(&cfg, &inv).expect("build can");
    let reminders = ReminderStore::open(dir.path().join("reminders.json")).expect("open reminders");

    let state = Arc::new(AppState::new(cfg, spec, inv, audit, real_can, reminders));
    (state, dir)
}

/// Same as `make_state` but with `webtransport.enabled = true` so config_route
/// produces a non-None advert URL. Used by the `/api/config` contract test.
pub fn make_state_with_wt_advert() -> (SharedState, tempfile::TempDir) {
    let (state, dir) = make_state();
    // SharedState wraps an immutable Arc; clone the inner config and rebuild.
    let mut cfg = state.cfg.clone();
    cfg.webtransport.enabled = true;
    cfg.webtransport.bind = "127.0.0.1:4433".into();
    let new_state = Arc::new(AppState::new(
        cfg,
        state.spec.clone(),
        state.inventory.clone(),
        AuditLog::open(dir.path().join("audit2.jsonl")).unwrap(),
        state.real_can.clone(),
        ReminderStore::open(dir.path().join("reminders2.json")).unwrap(),
    ));
    (new_state, dir)
}

/// Seed `state.params` (which is normally seeded by `telemetry::spawn`)
/// without spinning up the periodic loop. Tests that hit GET params need this.
pub fn seed_params(state: &SharedState) {
    use rudydae::types::{ParamSnapshot, ParamValue};
    use std::collections::BTreeMap;

    let mut seeded: BTreeMap<String, ParamSnapshot> = BTreeMap::new();
    for motor in &state.inventory.motors {
        let mut values = BTreeMap::new();
        for (name, desc) in state.spec.catalog() {
            let default = match desc.ty.as_str() {
                "float" | "f32" | "f64" => serde_json::json!(0.0_f32),
                "uint8" | "u8" | "uint16" | "u16" | "uint32" | "u32" => serde_json::json!(0_u32),
                _ => serde_json::Value::Null,
            };
            values.insert(
                name.clone(),
                ParamValue {
                    name: name.clone(),
                    index: desc.index,
                    ty: desc.ty.clone(),
                    units: desc.units.clone(),
                    value: default,
                    hardware_range: desc.hardware_range,
                },
            );
        }
        seeded.insert(
            motor.role.clone(),
            ParamSnapshot {
                role: motor.role.clone(),
                values,
            },
        );
    }
    *state.params.write().expect("params") = seeded;
}

/// Seed one synthetic feedback row for every motor so /motors/:role/feedback
/// returns 200 without spinning up the mock CAN ticker.
pub fn seed_feedback(state: &SharedState) {
    use rudydae::types::MotorFeedback;
    let mut latest = state.latest.write().expect("latest");
    for motor in &state.inventory.motors {
        latest.insert(
            motor.role.clone(),
            MotorFeedback {
                t_ms: 1_700_000_000_000,
                role: motor.role.clone(),
                can_id: motor.can_id,
                mech_pos_rad: 0.1,
                mech_vel_rad_s: 0.0,
                torque_nm: 0.0,
                vbus_v: 48.0,
                temp_c: 30.0,
                fault_sta: 0,
                warn_sta: 0,
            },
        );
    }
}

/// Suppress a clippy::dead_code-style warning if `Write` isn't used; some
/// tests pull this module via `mod common` but only use a subset.
#[allow(dead_code)]
fn _force_write_used(_w: &mut dyn Write) {}
