use std::path::PathBuf;

use super::*;

fn cfg_with(audit_log: &str, db_path: Option<&str>) -> Config {
    Config {
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
            actuator_spec: PathBuf::from("spec.yaml"),
            inventory: PathBuf::from("inv.yaml"),
            inventory_seed: None,
            audit_log: PathBuf::from(audit_log),
        },
        can: CanConfig {
            mock: true,
            buses: vec![],
        },
        telemetry: TelemetryConfig {
            poll_interval_ms: super::telemetry::default_poll_ms(),
        },
        safety: SafetyConfig {
            require_verified: true,
            boot_max_step_rad: super::safety::default_boot_max_step_rad(),
            step_size_rad: super::safety::default_step_size_rad(),
            tick_interval_ms: super::safety::default_tick_interval_ms(),
            homing_speed_rad_s: None,
            tracking_error_max_rad: super::safety::default_tracking_error_max_rad(),
            tracking_error_grace_ticks: super::safety::default_tracking_error_grace_ticks(),
            tracking_freshness_max_age_ms: super::safety::default_tracking_freshness_max_age_ms(),
            tracking_error_debounce_ticks: super::safety::default_tracking_error_debounce_ticks(),
            band_violation_debounce_ticks: super::safety::default_band_violation_debounce_ticks(),
            boot_tracking_error_max_rad: super::safety::default_boot_tracking_error_max_rad(),
            target_tolerance_rad: super::safety::default_target_tolerance_rad(),
            target_dwell_ticks: super::safety::default_target_dwell_ticks(),
            homer_timeout_ms: super::safety::default_homer_timeout_ms(),
            max_feedback_age_ms: super::safety::default_max_feedback_age_ms(),
            commission_readback_tolerance_rad:
                super::safety::default_commission_readback_tolerance_rad(),
            auto_home_on_boot: true,
            scan_on_boot: true,
            hold_kp_nm_per_rad: super::safety::default_hold_kp_nm_per_rad(),
            hold_kd_nm_s_per_rad: super::safety::default_hold_kd_nm_s_per_rad(),
        },
        logs: LogsConfig {
            db_path: db_path
                .map(PathBuf::from)
                .unwrap_or_else(super::logs::default_logs_db_path),
            ..Default::default()
        },
    }
}

#[test]
fn safety_config_json_roundtrip_preserves_tracking_gates() {
    let s = cfg_with("/tmp/audit.jsonl", None).safety;
    let json = serde_json::to_string(&s).expect("serialize safety");
    let back: SafetyConfig = serde_json::from_str(&json).expect("deserialize safety");
    assert_eq!(
        back.tracking_freshness_max_age_ms,
        super::safety::default_tracking_freshness_max_age_ms()
    );
    assert_eq!(
        back.tracking_error_debounce_ticks,
        super::safety::default_tracking_error_debounce_ticks()
    );
    assert_eq!(
        back.band_violation_debounce_ticks,
        super::safety::default_band_violation_debounce_ticks()
    );
    assert_eq!(
        back.tracking_error_grace_ticks,
        super::safety::default_tracking_error_grace_ticks()
    );
}

#[test]
fn actuator_common_roundtrip_preserves_active_report_persisted_flag() {
    let common = crate::inventory::ActuatorCommon {
        role: "test.motor".into(),
        can_bus: "can0".into(),
        can_id: 7,
        present: true,
        verified: false,
        commissioned_at: None,
        firmware_version: None,
        travel_limits: None,
        commissioned_zero_offset: None,
        active_report_persisted: true,
        predefined_home_rad: None,
        homing_speed_rad_s: None,
        hold_kp_nm_per_rad: None,
        hold_kd_nm_s_per_rad: None,
        limb: None,
        joint_kind: None,
        notes_yaml: None,
        desired_params: std::collections::BTreeMap::new(),
        direction_sign: 1,
    };
    let json = serde_json::to_string(&common).expect("serialize actuator common");
    let back: crate::inventory::ActuatorCommon =
        serde_json::from_str(&json).expect("deserialize actuator common");
    assert!(back.active_report_persisted);
}

#[test]
fn normalize_relative_db_path_anchors_to_absolute_audit_log_parent() {
    // Pi-shaped config: absolute audit log + the relative default db_path
    // (i.e. operator never wrote a `[logs]` section). The fix must
    // re-home the SQLite DB next to the audit log so it lands on the
    // writable StateDirectory instead of the read-only release tree.
    let mut cfg = cfg_with("/var/lib/rudy/audit.jsonl", None);
    cfg.normalize_paths();
    assert_eq!(cfg.logs.db_path, PathBuf::from("/var/lib/rudy/logs.db"));
}

#[test]
fn normalize_keeps_dev_relative_paths_unchanged() {
    // Dev workflow: cwd is the repo root and the audit log is
    // intentionally relative. We must not turn that into an absolute
    // path or the DB would land outside the repo.
    let mut cfg = cfg_with("./.cortex/audit.jsonl", None);
    cfg.normalize_paths();
    assert_eq!(cfg.logs.db_path, PathBuf::from(".cortex/logs.db"));
}

#[test]
fn normalize_respects_explicit_absolute_db_path() {
    // Operator explicitly chose a different absolute db_path; we leave
    // it alone even if it doesn't share the audit log's parent.
    let mut cfg = cfg_with("/var/lib/rudy/audit.jsonl", Some("/srv/logs/rudy.db"));
    cfg.normalize_paths();
    assert_eq!(cfg.logs.db_path, PathBuf::from("/srv/logs/rudy.db"));
}
