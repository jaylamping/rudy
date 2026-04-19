//! Per-actuator soft travel-limits enforcement.
//!
//! Reads the live `Inventory` and rejects any commanded position that falls
//! outside the motor's `travel_limits` band. Reused by the jog endpoint
//! (today) and any future move-to / position-target endpoints (tomorrow).
//!
//! The hardware envelope (±4π per RS03 spec) is the absolute outer rail for
//! validation in `validate_band` — both the operator UI and the daemon
//! refuse to write a band wider than that.

use anyhow::Result;

use crate::can::motion::{shortest_signed_delta, wrap_to_pi};
use crate::inventory::TravelLimits;
use crate::state::SharedState;
use crate::types::SafetyEvent;

/// Outer rail used to bound every `travel_limits` write. Matches the RS03
/// MIT-mode position-control encoding (`op_control_scaling.position.range`
/// in `config/actuators/robstride_rs03.yaml`). A band wider than this would
/// be useless because the firmware can't even receive a setpoint outside
/// it.
pub const HARDWARE_POSITION_MIN_RAD: f32 = -4.0 * std::f32::consts::PI;
pub const HARDWARE_POSITION_MAX_RAD: f32 = 4.0 * std::f32::consts::PI;

/// Validate a candidate `[min_rad, max_rad]` band against the hardware outer
/// rail and basic monotonicity. Returns the static reason string the API
/// layer should surface verbatim to the SPA (or `Ok(())`).
pub fn validate_band(min_rad: f32, max_rad: f32) -> Result<(), &'static str> {
    if !min_rad.is_finite() || !max_rad.is_finite() {
        return Err("non-finite travel bound");
    }
    if min_rad >= max_rad {
        return Err("travel min must be strictly less than travel max");
    }
    if min_rad < HARDWARE_POSITION_MIN_RAD {
        return Err("travel min below hardware envelope");
    }
    if max_rad > HARDWARE_POSITION_MAX_RAD {
        return Err("travel max above hardware envelope");
    }
    Ok(())
}

/// Outcome of enforcing the band on a commanded position.
#[derive(Debug, Clone)]
pub enum BandCheck {
    /// No travel band on file → unrestricted (firmware envelope still applies).
    NoLimit,
    /// Inside the band; safe to forward. `delta_rad` is the shortest signed
    /// principal-angle distance from current to target, populated by
    /// `enforce_position_with_path` (zero from the simpler `enforce_position`).
    InBand {
        min_rad: f32,
        max_rad: f32,
        delta_rad: f32,
    },
    /// Target endpoint is outside the band. The caller should reject the
    /// request and audit-log; the enforcer already broadcast the
    /// corresponding `SafetyEvent`.
    OutOfBand {
        min_rad: f32,
        max_rad: f32,
        attempted_rad: f32,
    },
    /// Target endpoint is inside the band but the swept arc crosses the
    /// band boundary (current position is outside). Rejected by every
    /// motion endpoint EXCEPT the Layer 6 auto-recovery routine, which is
    /// allowed exactly this exception by design.
    PathViolation {
        min_rad: f32,
        max_rad: f32,
        current_rad: f32,
        target_rad: f32,
    },
}

/// Look up the travel band for `role` and check `target_rad` against it.
/// Broadcasts a `SafetyEvent::TravelLimitViolation` on rejection so the
/// dashboard can render the alert without polling.
///
/// Returns `Ok(BandCheck::OutOfBand)` rather than `Err` so handlers can
/// decide how to surface the rejection (e.g. as a 409 with structured
/// detail rather than a 500 with anyhow text).
pub fn enforce_position(state: &SharedState, role: &str, target_rad: f32) -> Result<BandCheck> {
    let limits: Option<TravelLimits> = state
        .inventory
        .read()
        .map_err(|_| anyhow::anyhow!("inventory poisoned"))?
        .by_role(role)
        .and_then(|m| m.travel_limits.clone());
    let Some(limits) = limits else {
        return Ok(BandCheck::NoLimit);
    };
    if target_rad < limits.min_rad || target_rad > limits.max_rad {
        let _ = state
            .safety_event_tx
            .send(SafetyEvent::TravelLimitViolation {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.to_string(),
                attempted_rad: target_rad,
                min_rad: limits.min_rad,
                max_rad: limits.max_rad,
            });
        return Ok(BandCheck::OutOfBand {
            min_rad: limits.min_rad,
            max_rad: limits.max_rad,
            attempted_rad: target_rad,
        });
    }
    Ok(BandCheck::InBand {
        min_rad: limits.min_rad,
        max_rad: limits.max_rad,
        delta_rad: 0.0,
    })
}

/// Principal-angle path-aware band check. Use this from any handler that
/// produces motion (jog, home, bench-tests-that-command-position).
///
/// Both `current_rad` and `target_rad` are reduced to principal angles in
/// [-pi, +pi] before the check. The check passes only when:
///
///  1. the principal target endpoint is inside `[min_rad, max_rad]`, AND
///  2. the principal current position is also inside the band — which, for
///     the < 360 deg cable-bound joints we have, guarantees the swept
///     shortest-path arc stays in band.
///
/// If only condition 1 holds (target in band but current outside), the
/// result is [`BandCheck::PathViolation`] — the swept arc would cross the
/// boundary. This is the chokepoint that prevents the multi-turn-encoder
/// disaster: the firmware might still take a long path, but a daemon that
/// commands "go to 0 deg" while reading "+20 deg (actually +20 deg + 360)"
/// is refused before any frame leaves the host.
pub fn enforce_position_with_path(
    state: &SharedState,
    role: &str,
    current_rad: f32,
    target_rad: f32,
) -> Result<BandCheck> {
    let limits: Option<TravelLimits> = state
        .inventory
        .read()
        .map_err(|_| anyhow::anyhow!("inventory poisoned"))?
        .by_role(role)
        .and_then(|m| m.travel_limits.clone());
    let Some(limits) = limits else {
        return Ok(BandCheck::NoLimit);
    };

    let cur_p = wrap_to_pi(current_rad);
    let tgt_p = wrap_to_pi(target_rad);

    if tgt_p < limits.min_rad || tgt_p > limits.max_rad {
        let _ = state
            .safety_event_tx
            .send(SafetyEvent::TravelLimitViolation {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.to_string(),
                attempted_rad: tgt_p,
                min_rad: limits.min_rad,
                max_rad: limits.max_rad,
            });
        return Ok(BandCheck::OutOfBand {
            min_rad: limits.min_rad,
            max_rad: limits.max_rad,
            attempted_rad: tgt_p,
        });
    }

    if cur_p < limits.min_rad || cur_p > limits.max_rad {
        let _ = state
            .safety_event_tx
            .send(SafetyEvent::TravelLimitViolation {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.to_string(),
                attempted_rad: cur_p,
                min_rad: limits.min_rad,
                max_rad: limits.max_rad,
            });
        return Ok(BandCheck::PathViolation {
            min_rad: limits.min_rad,
            max_rad: limits.max_rad,
            current_rad: cur_p,
            target_rad: tgt_p,
        });
    }

    Ok(BandCheck::InBand {
        min_rad: limits.min_rad,
        max_rad: limits.max_rad,
        delta_rad: shortest_signed_delta(cur_p, tgt_p),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_band_rejects_inverted_band() {
        assert!(validate_band(1.0, -1.0).is_err());
        assert!(validate_band(0.0, 0.0).is_err());
    }

    #[test]
    fn validate_band_rejects_non_finite() {
        assert!(validate_band(f32::NAN, 1.0).is_err());
        assert!(validate_band(0.0, f32::INFINITY).is_err());
    }

    #[test]
    fn validate_band_enforces_outer_rail() {
        assert!(validate_band(HARDWARE_POSITION_MIN_RAD - 0.01, 0.0).is_err());
        assert!(validate_band(0.0, HARDWARE_POSITION_MAX_RAD + 0.01).is_err());
    }

    #[test]
    fn validate_band_accepts_normal_band() {
        assert!(validate_band(-1.0, 1.0).is_ok());
    }
}

#[cfg(test)]
mod path_check_tests {
    //! Tests for `enforce_position_with_path` use a real `AppState` so the
    //! TravelLimits lookup goes through the same code path the production
    //! daemon does. Helper lives in the integration `tests/common`; here
    //! we duplicate a tiny subset to keep the unit test hermetic.

    use super::*;
    use crate::audit::AuditLog;
    use crate::can;
    use crate::config::{
        CanConfig, Config, HttpConfig, LogsConfig, PathsConfig, SafetyConfig, TelemetryConfig,
        WebTransportConfig,
    };
    use crate::inventory::Inventory;
    use crate::reminders::ReminderStore;
    use crate::spec::ActuatorSpec;
    use crate::state::AppState;
    use std::sync::Arc;

    fn state_with_band(min: f32, max: f32) -> (crate::state::SharedState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let spec_path = dir.path().join("spec.yaml");
        std::fs::write(
            &spec_path,
            "schema_version: 2\nactuator_model: T\nfirmware_limits: {}\nobservables: {}\n",
        )
        .unwrap();
        let inv_path = dir.path().join("inv.yaml");
        std::fs::write(
            &inv_path,
            format!(
                "schema_version: 1\nmotors:\n  - role: m\n    can_bus: can0\n    can_id: 1\n    travel_limits:\n      min_rad: {min}\n      max_rad: {max}\n"
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
                auto_recovery_max_rad: std::f32::consts::FRAC_PI_2,
                recovery_margin_rad: 0.087,
                step_size_rad: 0.02,
                tick_interval_ms: 5,
                tracking_error_max_rad: 0.05,
                target_tolerance_rad: 0.005,
                homer_timeout_ms: 5_000,
                auto_recovery_enabled: true,
                max_feedback_age_ms: 100,
            },
            logs: LogsConfig {
                db_path: dir.path().join("logs.db"),
                ..LogsConfig::default()
            },
        };
        let spec = ActuatorSpec::load(&spec_path).unwrap();
        let inv = Inventory::load(&inv_path).unwrap();
        let audit = AuditLog::open(dir.path().join("audit.jsonl")).unwrap();
        let real_can = can::build_handle(&cfg, &inv).unwrap();
        let reminders = ReminderStore::open(dir.path().join("reminders.json")).unwrap();
        (
            Arc::new(AppState::new(cfg, spec, inv, audit, real_can, reminders)),
            dir,
        )
    }

    #[test]
    fn path_check_in_band_returns_inband_with_delta() {
        let (s, _d) = state_with_band(-1.0, 1.0);
        let r = enforce_position_with_path(&s, "m", 0.0, 0.5).unwrap();
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
        let r = enforce_position_with_path(&s, "m", 0.0, 1.5).unwrap();
        assert!(matches!(r, BandCheck::OutOfBand { .. }));
    }

    #[test]
    fn path_check_current_outside_band_returns_pathviolation() {
        let (s, _d) = state_with_band(-1.0, 1.0);
        let r = enforce_position_with_path(&s, "m", 1.5, 0.0).unwrap();
        assert!(matches!(r, BandCheck::PathViolation { .. }));
    }

    #[test]
    fn path_check_no_band_returns_nolimit() {
        let dir = tempfile::tempdir().unwrap();
        let spec_path = dir.path().join("spec.yaml");
        std::fs::write(
            &spec_path,
            "schema_version: 2\nactuator_model: T\nfirmware_limits: {}\nobservables: {}\n",
        )
        .unwrap();
        let inv_path = dir.path().join("inv.yaml");
        std::fs::write(
            &inv_path,
            "schema_version: 1\nmotors:\n  - role: m\n    can_bus: can0\n    can_id: 1\n",
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
                auto_recovery_max_rad: std::f32::consts::FRAC_PI_2,
                recovery_margin_rad: 0.087,
                step_size_rad: 0.02,
                tick_interval_ms: 5,
                tracking_error_max_rad: 0.05,
                target_tolerance_rad: 0.005,
                homer_timeout_ms: 5_000,
                auto_recovery_enabled: true,
                max_feedback_age_ms: 100,
            },
            logs: LogsConfig {
                db_path: dir.path().join("logs.db"),
                ..LogsConfig::default()
            },
        };
        let spec = ActuatorSpec::load(&spec_path).unwrap();
        let inv = Inventory::load(&inv_path).unwrap();
        let audit = AuditLog::open(dir.path().join("audit.jsonl")).unwrap();
        let real_can = can::build_handle(&cfg, &inv).unwrap();
        let reminders = ReminderStore::open(dir.path().join("reminders.json")).unwrap();
        let s = Arc::new(AppState::new(cfg, spec, inv, audit, real_can, reminders));
        let r = enforce_position_with_path(&s, "m", 100.0, 100.0).unwrap();
        assert!(matches!(r, BandCheck::NoLimit));
    }
}
