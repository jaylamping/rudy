//! Shared test fixtures.
//!
//! Boots cortex's `AppState` against in-memory minimal YAML so tests don't
//! depend on the prod inventory (which intentionally has no `verified: true`
//! motors and grows over time).

#![allow(dead_code)] // helpers are imported per-test, some only by some tests

mod fixtures;

use std::io::Write;

use axum::response::Response;
use http_body_util::BodyExt;

pub use fixtures::{INVENTORY_YAML, SPEC_YAML};

/// Deserialize axum integration-test responses (shared by `tests/api/*.rs`).
pub async fn body_json<T: serde::de::DeserializeOwned>(resp: Response) -> T {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<T>(&bytes).unwrap_or_else(|e| {
        let s = std::str::from_utf8(&bytes).unwrap_or("<binary>");
        panic!("deserialise failed: {e}; body was: {s}");
    })
}
use std::sync::Arc;

use cortex::audit::AuditLog;
use cortex::can;
use cortex::config::{
    CanConfig, Config, HttpConfig, LogsConfig, PathsConfig, SafetyConfig, TelemetryConfig,
    WebTransportConfig,
};
use cortex::inventory::{Actuator, Device, Inventory};
use cortex::reminders::ReminderStore;
use cortex::spec;
use cortex::state::{AppState, SharedState};

/// Build a temp-rooted SharedState with mock CAN, no TLS, no WT (so we don't
/// need cert files). The audit log goes to the per-test `tempdir`.
pub fn make_state() -> (SharedState, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");

    let spec_path = dir.path().join("robstride_rs03.yaml");
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
            inventory_seed: None,
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
            boot_max_step_rad: 0.087,
            step_size_rad: 0.02,
            tick_interval_ms: 5,
            homing_speed_rad_s: None,
            tracking_error_max_rad: 0.05,
            // Tests exercise mock-CAN where measurement is simulated to
            // perfectly track the setpoint, so the cold-motor
            // grace-window doesn't matter — keep it at 0 to preserve
            // pre-existing tracking-error abort timing.
            tracking_error_grace_ticks: 0,
            tracking_freshness_max_age_ms: 100,
            tracking_error_debounce_ticks: 3,
            band_violation_debounce_ticks: 3,
            // Match the operator-driven budget so boot-orchestrator
            // tests that exercise abort paths continue to fire on the
            // same thresholds they always have.
            boot_tracking_error_max_rad: 0.05,
            target_tolerance_rad: 0.005,
            target_dwell_ticks: 5,
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
            retention_days: 7,
            default_filter: "cortex=info".into(),
            batch_max_rows: 64,
            batch_flush_ms: 25,
            purge_interval_s: 60,
        },
    };

    let specs = spec::load_robstride_specs(dir.path(), Some(&spec_path)).expect("load specs");
    let inv = Inventory::load(&inv_path).expect("load inventory");
    let audit = AuditLog::open(&audit_path).expect("open audit");
    let real_can = can::build_handle(&cfg, &inv).expect("build can");
    let reminders = ReminderStore::open(dir.path().join("reminders.json")).expect("open reminders");

    let state = Arc::new(AppState::new(cfg, specs, inv, audit, real_can, reminders));
    (state, dir)
}

/// Mutate a single actuator row by `role` (test helper; inventory is v2 `devices:`).
pub fn actuator_mut<'a>(inv: &'a mut Inventory, role: &str) -> Option<&'a mut Actuator> {
    inv.devices.iter_mut().find_map(|d| {
        if let Device::Actuator(a) = d {
            if a.common.role == role {
                return Some(a);
            }
        }
        None
    })
}

/// Non-Linux only: same disk layout as [`make_state`], but
/// `state.real_can = Some(Arc::new(RealCanHandle))` so `POST /commission` runs
/// the CAN branch (`set_zero` → …). The non-Linux [`can::RealCanHandle`] stub
/// always fails `set_zero`, so commission aborts before any inventory write.
#[cfg(not(target_os = "linux"))]
pub fn make_state_commission_can_path_fails() -> (SharedState, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");

    let spec_path = dir.path().join("robstride_rs03.yaml");
    std::fs::write(&spec_path, SPEC_YAML).unwrap();

    let inv_path = dir.path().join("inventory.yaml");
    std::fs::write(&inv_path, INVENTORY_YAML).unwrap();

    let audit_path = dir.path().join("audit.jsonl");

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
            boot_max_step_rad: 0.087,
            step_size_rad: 0.02,
            tick_interval_ms: 5,
            homing_speed_rad_s: None,
            tracking_error_max_rad: 0.05,
            tracking_error_grace_ticks: 0,
            tracking_freshness_max_age_ms: 100,
            tracking_error_debounce_ticks: 3,
            band_violation_debounce_ticks: 3,
            boot_tracking_error_max_rad: 0.05,
            target_tolerance_rad: 0.005,
            target_dwell_ticks: 5,
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
            retention_days: 7,
            default_filter: "cortex=info".into(),
            batch_max_rows: 64,
            batch_flush_ms: 25,
            purge_interval_s: 60,
        },
    };

    let specs = spec::load_robstride_specs(dir.path(), Some(&spec_path)).expect("load specs");
    let inv = Inventory::load(&inv_path).expect("load inventory");
    let audit = AuditLog::open(&audit_path).expect("open audit");
    let real_can = Some(Arc::new(can::RealCanHandle));
    let reminders = ReminderStore::open(dir.path().join("reminders.json")).expect("open reminders");

    let state = Arc::new(AppState::new(cfg, specs, inv, audit, real_can, reminders));
    (state, dir)
}

/// Same as [`make_state`] but toggles `safety.auto_home_on_boot` (rebuilds
/// `AppState` because `SharedState` is immutable).
pub fn make_state_auto_home_on_boot(auto_home_on_boot: bool) -> (SharedState, tempfile::TempDir) {
    let (state, dir) = make_state();
    let mut cfg = state.cfg.clone();
    cfg.safety.auto_home_on_boot = auto_home_on_boot;
    let audit_path = dir.path().join("audit_auto_home.jsonl");
    cfg.paths.audit_log = audit_path.clone();
    let inv = state.inventory.read().expect("inventory poisoned").clone();
    let new_state = Arc::new(AppState::new(
        cfg,
        state.specs.clone(),
        inv,
        AuditLog::open(&audit_path).unwrap(),
        state.real_can.clone(),
        ReminderStore::open(dir.path().join("reminders_auto_home.json")).unwrap(),
    ));
    (new_state, dir)
}

/// Same as [`make_state`] but uses a slow homer tick and the minimum enforced
/// homer wall-clock timeout (`home_ramp` clamps to 1000 ms) so
/// `home_ramp::run` reliably hits `"timeout"` in mock-CAN tests without
/// relying on tracking-error or path violations.
pub fn make_state_homer_times_out_quickly() -> (SharedState, tempfile::TempDir) {
    let (state, dir) = make_state();
    let mut cfg = state.cfg.clone();
    cfg.safety.tick_interval_ms = 500;
    cfg.safety.homer_timeout_ms = 1_000;
    let audit_path = dir.path().join("audit_homer_timeout.jsonl");
    cfg.paths.audit_log = audit_path.clone();
    let inv = state.inventory.read().expect("inventory poisoned").clone();
    let new_state = Arc::new(AppState::new(
        cfg,
        state.specs.clone(),
        inv,
        AuditLog::open(&audit_path).unwrap(),
        state.real_can.clone(),
        ReminderStore::open(dir.path().join("reminders_homer_timeout.json")).unwrap(),
    ));
    (new_state, dir)
}

/// Set in-memory `travel_limits` for a motor (same pattern as `motion_lifecycle`).
pub fn set_travel_limits(state: &SharedState, role: &str, min_rad: f32, max_rad: f32) {
    use cortex::inventory::TravelLimits;
    let mut inv = state.inventory.write().expect("inventory poisoned");
    let mut found = false;
    for d in &mut inv.devices {
        if let Device::Actuator(a) = d {
            if a.common.role == role {
                a.common.travel_limits = Some(TravelLimits {
                    min_rad,
                    max_rad,
                    updated_at: None,
                });
                found = true;
                break;
            }
        }
    }
    assert!(found, "inventory missing role {role}");
}

/// Same as `make_state` but with `webtransport.enabled = true` so config_route
/// produces a non-None advert URL. Used by the `/api/config` contract test.
pub fn make_state_with_wt_advert() -> (SharedState, tempfile::TempDir) {
    let (state, dir) = make_state();
    // SharedState wraps an immutable Arc; clone the inner config and rebuild.
    let mut cfg = state.cfg.clone();
    cfg.webtransport.enabled = true;
    cfg.webtransport.bind = "127.0.0.1:4433".into();
    let audit_path = dir.path().join("audit2.jsonl");
    cfg.paths.audit_log = audit_path.clone();
    let inv = state.inventory.read().expect("inventory poisoned").clone();
    let new_state = Arc::new(AppState::new(
        cfg,
        state.specs.clone(),
        inv,
        AuditLog::open(&audit_path).unwrap(),
        state.real_can.clone(),
        ReminderStore::open(dir.path().join("reminders2.json")).unwrap(),
    ));
    (new_state, dir)
}

/// Seed `state.params` (which is normally seeded by `telemetry::spawn`)
/// without spinning up the periodic loop. Tests that hit GET params need this.
pub fn seed_params(state: &SharedState) {
    use cortex::types::{ParamSnapshot, ParamValue};
    use std::collections::BTreeMap;

    let mut seeded: BTreeMap<String, ParamSnapshot> = BTreeMap::new();
    let inv = state.inventory.read().expect("inventory poisoned");
    for motor in inv.actuators() {
        let mut values = BTreeMap::new();
        let spec = state.spec_for(motor.robstride_model());
        for (name, desc, writable) in spec.catalog() {
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
                    writable,
                    desired: None,
                    drift: None,
                },
            );
        }
        seeded.insert(
            motor.common.role.clone(),
            ParamSnapshot {
                role: motor.common.role.clone(),
                values,
            },
        );
    }
    *state.params.write().expect("params") = seeded;
}

/// Seed one synthetic feedback row for every motor so /motors/:role/feedback
/// returns 200 without spinning up the mock CAN ticker.
///
/// `t_ms` is "now" so jog's stale-feedback guard
/// (`safety.max_feedback_age_ms`, default 100 ms) treats the row as live.
/// Tests that need to exercise the stale path should overwrite the row
/// after this call with an old `t_ms`.
pub fn seed_feedback(state: &SharedState) {
    use cortex::types::MotorFeedback;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut latest = state.latest.write().expect("latest");
    let inv = state.inventory.read().expect("inventory poisoned");
    for motor in inv.actuators() {
        latest.insert(
            motor.common.role.clone(),
            MotorFeedback {
                t_ms: now_ms,
                role: motor.common.role.clone(),
                can_id: motor.common.can_id,
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

/// Mock CAN fixtures do not advance `state.latest[*]` during wall-clock sleeps
/// inside the homer (`finish_home_success` waits 500 ms before hold
/// verification). On non-Linux, `real_can` is `None` so the home-ramp loop
/// uses perfect tracking in RAM (`last_measured = setpoint_unwrapped`) but
/// **never rewrites** `state.latest[*].mech_pos_rad`, which still holds the
/// pre-home seed (often `0.1` from [`seed_feedback`]). Real hardware keeps
/// publishing type-2 feedback at the true joint angle, so hold verification
/// reads fresh `t_ms` **and** a `mech_pos` consistent with the home target.
///
/// `mech_pos_rad_reported` should match the principal home angle the
/// orchestrator ramps to (typically `0.0`, or `predefined_home_rad` when set).
pub fn spawn_latest_timestamp_refresh(
    state: SharedState,
    role: impl Into<String>,
    mech_pos_rad_reported: f32,
) -> tokio::task::JoinHandle<()> {
    let role = role.into();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let now = chrono::Utc::now().timestamp_millis();
            let mut w = state.latest.write().expect("latest poisoned");
            if let Some(fb) = w.get_mut(&role) {
                fb.t_ms = now;
                fb.mech_pos_rad = mech_pos_rad_reported;
            }
        }
    })
}

/// Force every motor's boot state to `Homed`. Call from tests whose intent
/// pre-dates the boot-time gate so they don't trip the new enable
/// preconditions; tests for the gate itself should NOT use this.
pub fn force_homed(state: &SharedState) {
    use cortex::boot_state::BootState;
    let mut bs = state.boot_state.write().expect("boot_state");
    let inv = state.inventory.read().expect("inventory poisoned");
    for m in inv.actuators() {
        bs.insert(m.common.role.clone(), BootState::Homed);
    }
}

/// Seed boot state for a single motor.
pub fn set_boot_state(state: &SharedState, role: &str, bs: cortex::boot_state::BootState) {
    state
        .boot_state
        .write()
        .expect("boot_state")
        .insert(role.into(), bs);
}

/// Suppress a clippy::dead_code-style warning if `Write` isn't used; some
/// tests pull this module via `mod common` but only use a subset.
#[allow(dead_code)]
fn _force_write_used(_w: &mut dyn Write) {}
