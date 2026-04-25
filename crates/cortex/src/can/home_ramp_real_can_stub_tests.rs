//! End-to-end coverage of the home-ramp loop with `real_can = Some`,
//! using the in-tree `RealCanHandle` stub (`set_velocity_setpoint` /
//! `stop` are no-op `Ok`). Builds only on non-Linux CI hosts to match
//! the sibling stub's `#[cfg]` gate.
//!
//! These tests stress the *control-flow* of the loop — gates, debounce
//! counters, success tolerance — by wedging or scripting
//! `state.latest[role].mech_pos_rad` from a side task. The CAN bus is
//! never touched, so any test that depends on the firmware actually
//! responding to the velocity command (e.g. proving `lag_scale` brakes
//! a real motor) needs to live in a hardware-in-the-loop harness, not
//! here.

use std::sync::Arc;
use std::time::Duration;

use super::{resolve_homing_speed, run_with_tracking_budget, MAX_HOMER_VEL_RAD_S};
use crate::audit::AuditLog;
use crate::can;
use crate::config::{
    CanConfig, Config, HttpConfig, LogsConfig, PathsConfig, RuntimeDbConfig, SafetyConfig,
    TelemetryConfig, WebTransportConfig,
};
use crate::inventory::Inventory;
use crate::reminders::ReminderStore;
use crate::spec;
use crate::state::AppState;
use crate::types::MotorFeedback;

fn state_with_real_can_stub_inner(
    safety_homing: Option<f32>,
) -> (crate::state::SharedState, crate::inventory::Actuator) {
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
            homing_speed_rad_s: safety_homing,
            tracking_error_max_rad: 0.05,
            tracking_error_grace_ticks: 0,
            tracking_freshness_max_age_ms: 100,
            tracking_error_debounce_ticks: 3,
            band_violation_debounce_ticks: 3,
            boot_tracking_error_max_rad: 0.05,
            target_tolerance_rad: 0.005,
            target_dwell_ticks: 1,
            // Stub tests script mech_pos_rad directly and don't drive
            // mech_vel_rad_s off the simulated position trajectory, so
            // the default value (0.0) would satisfy any finite threshold.
            // Kept as a huge value so anyone reading the test can tell
            // the velocity gate isn't part of what's being pinned here.
            target_dwell_max_vel_rad_s: f32::INFINITY,
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
        runtime: RuntimeDbConfig::default(),
    };
    let specs = spec::load_robstride_specs(dir.path(), Some(&spec_path)).unwrap();
    let inv = Inventory::load(&inv_path).unwrap();
    let motor = inv.actuators().next().cloned().expect("fixture actuator");
    let audit = AuditLog::open(dir.path().join("audit.jsonl")).unwrap();
    let real_can = Some(Arc::new(can::RealCanHandle));
    let reminders = ReminderStore::open(dir.path().join("reminders.json")).unwrap();
    let state = Arc::new(AppState::new(cfg, specs, inv, audit, real_can, reminders));
    (state, motor)
}

fn state_with_real_can_stub() -> (crate::state::SharedState, crate::inventory::Actuator) {
    state_with_real_can_stub_inner(None)
}

/// Velocity-gate variant of the stub fixture. Overrides
/// `target_dwell_max_vel_rad_s` (default INFINITY disables the gate and
/// preserves legacy position-only dwell) and tightens `homer_timeout_ms`
/// so a test that expects the gate to BLOCK success returns inside a
/// reasonable wall-clock.
fn state_with_velocity_gate(
    dwell_max_vel_rad_s: f32,
    homer_timeout_ms: u32,
) -> (crate::state::SharedState, crate::inventory::Actuator) {
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
            band_violation_debounce_ticks: 3,
            boot_tracking_error_max_rad: 0.05,
            target_tolerance_rad: 0.005,
            target_dwell_ticks: 1,
            // The whole point of this fixture: exercise the finite-positive
            // branch of the velocity gate instead of the INFINITY fallback.
            target_dwell_max_vel_rad_s: dwell_max_vel_rad_s,
            homer_timeout_ms,
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
        runtime: RuntimeDbConfig::default(),
    };
    let specs = spec::load_robstride_specs(dir.path(), Some(&spec_path)).unwrap();
    let inv = Inventory::load(&inv_path).unwrap();
    let motor = inv.actuators().next().cloned().expect("fixture actuator");
    let audit = AuditLog::open(dir.path().join("audit.jsonl")).unwrap();
    let real_can = Some(Arc::new(can::RealCanHandle));
    let reminders = ReminderStore::open(dir.path().join("reminders.json")).unwrap();
    let state = Arc::new(AppState::new(cfg, specs, inv, audit, real_can, reminders));
    (state, motor)
}

#[test]
fn resolve_homing_speed_uses_actuator_override() {
    let (state, mut motor) = state_with_real_can_stub();
    motor.common.homing_speed_rad_s = Some(0.35);
    let (v, src) = resolve_homing_speed(&state, &motor);
    assert!((v - 0.35).abs() < 1e-5, "got {v}");
    assert_eq!(src, "actuator_override");
}

#[test]
fn resolve_homing_speed_clamps_high_override() {
    let (state, mut motor) = state_with_real_can_stub();
    motor.common.homing_speed_rad_s = Some(MAX_HOMER_VEL_RAD_S + 1.0);
    let (v, src) = resolve_homing_speed(&state, &motor);
    assert!((v - MAX_HOMER_VEL_RAD_S).abs() < 1e-5, "got {v}");
    assert_eq!(src, "actuator_override");
}

#[test]
fn resolve_homing_speed_derives_from_step_when_global_unset() {
    let (state, mut motor) = state_with_real_can_stub();
    motor.common.homing_speed_rad_s = None;
    let (v, src) = resolve_homing_speed(&state, &motor);
    // step_size_rad 0.02 / 0.005s = 4 rad/s -> clamped to MAX_HOMER_VEL_RAD_S
    assert!((v - MAX_HOMER_VEL_RAD_S).abs() < 1e-5, "got {v}");
    assert_eq!(src, "derived_step_tick");
}

#[test]
fn resolve_homing_speed_uses_explicit_global() {
    let (state, mut motor) = state_with_real_can_stub_inner(Some(0.25));
    motor.common.homing_speed_rad_s = None;
    let (v, src) = resolve_homing_speed(&state, &motor);
    assert!((v - 0.25).abs() < 1e-5, "got {v}");
    assert_eq!(src, "global_config");
}

#[test]
fn resolve_homing_speed_non_positive_override_falls_back_to_global() {
    let (state, mut motor) = state_with_real_can_stub_inner(Some(0.25));
    motor.common.homing_speed_rad_s = Some(0.0);
    let (v, src) = resolve_homing_speed(&state, &motor);
    assert!((v - 0.25).abs() < 1e-5, "got {v}");
    assert_eq!(src, "global_config");
}

#[tokio::test]
async fn stuck_motor_aborts_after_debounced_tracking_error() {
    let (state, motor) = state_with_real_can_stub();
    let role = motor.common.role.clone();
    {
        let mut latest = state.latest.write().unwrap();
        latest.insert(
            role.clone(),
            MotorFeedback {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.clone(),
                can_id: 1,
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
    let updater = {
        let state = state.clone();
        let role = role.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(2)).await;
                let mut w = state.latest.write().unwrap();
                if let Some(fb) = w.get_mut(&role) {
                    fb.t_ms = chrono::Utc::now().timestamp_millis();
                    fb.mech_pos_rad = 0.0;
                }
            }
        })
    };
    let r = run_with_tracking_budget(state.clone(), motor, 0.0, 0.5, 0.05).await;
    updater.abort();
    let Err((reason, _)) = r else {
        panic!("expected Err, got {r:?}");
    };
    assert_eq!(reason, "tracking_error");
}

#[tokio::test]
async fn sustained_out_of_band_aborts_with_path_violation_after_debounce() {
    // Pin both halves of the band-violation hardening:
    //
    //   1. The debounce gate doesn't fire on a single tick of OOB
    //      telemetry (the motor "just barely" past the edge after a
    //      single overshoot — exactly the failure mode that
    //      shoulder_pitch's auto-home tripped).
    //   2. Sustained OOB still aborts with `path_violation`, not
    //      `tracking_error` or `timeout`. The whole point of the
    //      debounce is to give the *reactive* velocity flip a few
    //      ticks to recover; a motor that genuinely refuses to come
    //      back into band must still surface as `path_violation` so
    //      the operator-recovery flow runs the right script.
    //
    // We wedge `mech_pos_rad` at -1.1 (band is [-1.0, +1.0], from the
    // fixture YAML), which is a sustained OOB position. The homer is
    // asked to drive to 0.0; the home-ramp's velocity cap pulls vel
    // toward zero (because `dist_to_edge = 0` at this measured
    // position), the band-debounce counter increments every fresh
    // tick, and after three consecutive fresh ticks the abort fires.
    let (state, motor) = state_with_real_can_stub();
    let role = motor.common.role.clone();
    {
        let mut latest = state.latest.write().unwrap();
        latest.insert(
            role.clone(),
            MotorFeedback {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.clone(),
                can_id: 1,
                mech_pos_rad: -1.1,
                mech_vel_rad_s: 0.0,
                torque_nm: 0.0,
                vbus_v: 48.0,
                temp_c: 30.0,
                fault_sta: 0,
                warn_sta: 0,
            },
        );
    }
    let updater = {
        let state = state.clone();
        let role = role.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(2)).await;
                let mut w = state.latest.write().unwrap();
                if let Some(fb) = w.get_mut(&role) {
                    fb.t_ms = chrono::Utc::now().timestamp_millis();
                    // Wedged just outside the lower band edge.
                }
            }
        })
    };
    // Loose tracking budget so the tracking-error gate doesn't race
    // the band-violation gate to the abort. We're pinning the band
    // path specifically.
    let r = run_with_tracking_budget(state.clone(), motor, -1.1, 0.0, 1.0).await;
    updater.abort();
    let Err((reason, last_pos)) = r else {
        panic!("expected Err, got {r:?}");
    };
    assert_eq!(reason, "path_violation");
    // The reported `last_pos` should reflect the wedged measured
    // position, NOT the home target. Operators triage from this value
    // so it has to be the actual physical readout that tripped the
    // band check.
    assert!(
        (last_pos - -1.1_f32).abs() < 1e-3,
        "expected last_pos ~= -1.1, got {last_pos}"
    );
}

#[tokio::test]
async fn motor_outrunning_setpoint_does_not_trip_tracking() {
    // Pin the one-sided tracking-error gate. A gravity-assisted joint
    // (shoulder_pitch falling toward its low-gravity neutral pose
    // under a payload) advances faster than `nominal_speed`, so
    // `last_measured` runs AHEAD of the virtual setpoint in the
    // direction of motion. Pre-fix this tripped `tracking_error`
    // after grace+debounce ticks because the gate took `.abs()` of
    // the signed delta — which counted overrun and lag the same.
    // Post-fix only the lag side feeds the gate, so a steadily
    // outrunning motor must reach the home target without aborting.
    //
    // Test setup notes for future readers:
    //   - Fixture sets `step_size_rad = 0.02`, `tick_interval_ms = 5`
    //     → setpoint advances 0.02 rad/tick. We simulate the motor
    //     advancing 0.05 rad/tick (≈2.5x), so it reliably runs past
    //     setpoint by > 0.05 (the tracking budget) within a handful
    //     of ticks — well past `debounce_ticks = 3`.
    //   - The CAN handle is a stub; `set_velocity_setpoint` is a
    //     no-op. The "motor" is the updater task below, so the
    //     `lag_scale` velocity damping doesn't actually slow the
    //     simulated motor — that's deliberate. This test stresses
    //     the gate, not the damping.
    //   - Target = 0.5 with `from_rad = 1.0`; the simulated motor
    //     reaches the target around tick ~10, well before the 5s
    //     `homer_timeout_ms`.
    let (state, motor) = state_with_real_can_stub();
    let role = motor.common.role.clone();
    {
        let mut latest = state.latest.write().unwrap();
        latest.insert(
            role.clone(),
            MotorFeedback {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.clone(),
                can_id: 1,
                mech_pos_rad: 1.0,
                mech_vel_rad_s: 0.0,
                torque_nm: 0.0,
                vbus_v: 48.0,
                temp_c: 30.0,
                fault_sta: 0,
                warn_sta: 0,
            },
        );
    }
    let updater = {
        let state = state.clone();
        let role = role.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(2)).await;
                let mut w = state.latest.write().unwrap();
                if let Some(fb) = w.get_mut(&role) {
                    fb.t_ms = chrono::Utc::now().timestamp_millis();
                    // Drive measured toward 0.5 at 0.05 rad/tick
                    // (faster than setpoint's 0.02 rad/tick), then
                    // park at 0.5 so the success-tolerance check can
                    // fire.
                    if fb.mech_pos_rad > 0.5 {
                        fb.mech_pos_rad = (fb.mech_pos_rad - 0.05).max(0.5);
                    }
                }
            }
        })
    };
    let r = run_with_tracking_budget(state.clone(), motor, 1.0, 0.5, 0.05).await;
    updater.abort();
    assert!(
        r.is_ok(),
        "expected Ok (motor outran setpoint but stayed in band), got {r:?}"
    );
}

#[tokio::test]
async fn motor_landing_in_tolerance_breaks_immediately_no_bounce() {
    // Pin the early in-tolerance break that fixes the audible
    // "vibrate/bounce" at the end of an auto-home. Pre-fix, the
    // success check sat at the BOTTOM of the loop body, so a tick
    // where the motor first crossed into the deadband would still
    // recompute `direction` (which can flip if the motor overshot the
    // target by < tolerance) and command another tapered velocity in
    // the OPPOSITE direction. The motor would oscillate around the
    // home pose for several ticks until natural decay landed it
    // squarely enough inside the tolerance for the bottom-of-loop
    // check to evaluate Ok.
    //
    // Post-fix the check runs at the TOP of the tick, gated on fresh
    // telemetry, so the very first tick whose `last_measured` lands in
    // the deadband exits Ok before any other velocity command goes
    // out. This test pins:
    //
    //   1. The homer exits Ok within a small bounded number of ticks
    //      once the simulated motor parks inside the tolerance window
    //      (proving the early break fires, not the bottom check after
    //      a long bounce).
    //   2. The reported `final_pos` is the in-tolerance measured
    //      position, not the target value (proving we returned actual
    //      telemetry rather than a synthetic value from mock-mode).
    //
    // Setup: fixture target_tolerance_rad = 0.005, step_size_rad = 0.02,
    // tick_interval_ms = 5. We start the motor at 0.0 with target 0.10,
    // simulate it parking at 0.099 (inside the 0.005 tolerance from
    // 0.10), and assert Ok lands within ~30 ticks (well below the 5 s
    // / 1000-tick timeout).
    let (state, motor) = state_with_real_can_stub();
    let role = motor.common.role.clone();
    {
        let mut latest = state.latest.write().unwrap();
        latest.insert(
            role.clone(),
            MotorFeedback {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.clone(),
                can_id: 1,
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
    let updater = {
        let state = state.clone();
        let role = role.clone();
        tokio::spawn(async move {
            // Drive measured to 0.099 (inside the 0.005 tolerance from
            // the 0.10 target), then hold there. Without the early
            // break, the homer would wedge `direction` to ±1 each
            // tick depending on which side of 0.10 the previous
            // velocity command happened to push the motor — but we
            // only ever advertise 0.099 here, so the early break is
            // the ONLY path that can produce Ok.
            loop {
                tokio::time::sleep(Duration::from_millis(2)).await;
                let mut w = state.latest.write().unwrap();
                if let Some(fb) = w.get_mut(&role) {
                    fb.t_ms = chrono::Utc::now().timestamp_millis();
                    fb.mech_pos_rad = (fb.mech_pos_rad + 0.02).min(0.099);
                }
            }
        })
    };
    let r = run_with_tracking_budget(state.clone(), motor, 0.0, 0.10, 0.05).await;
    updater.abort();
    let Ok((final_pos, ticks)) = r else {
        panic!("expected Ok (motor reached tolerance band), got {r:?}");
    };
    // Generous upper bound: with 5 ms ticks and motor ramping at 0.02
    // rad/tick, it takes ~5 ticks of physical motion to cross 0.099.
    // 60 ticks gives plenty of headroom for the grace_ticks=0 fixture
    // and any spawn-blocking jitter while still aborting if the early
    // break regresses and we fall through to the natural-decay path.
    assert!(
        ticks <= 60,
        "expected early break within ~60 ticks, got {ticks} (suggests bounce regression)"
    );
    // The home-ramp now floors the success tolerance at 2.5 × step_size
    // (see `effective_tolerance_rad` in `home_ramp.rs`) to keep the dwell
    // gate satisfiable at high homing speeds. With this fixture's
    // step_size_rad = 0.02, the effective tolerance is 0.05, so the early
    // break fires at the first measured sample whose distance from target
    // (0.10) is < 0.05 — i.e. anywhere in [0.05, 0.099]. The updater
    // ramps in 0.02 increments capped at 0.099, so `final_pos` lands
    // somewhere in that band depending on tick timing. Pin the property
    // (in-band, below the cap) rather than the exact value.
    assert!(
        (0.05..=0.099).contains(&final_pos),
        "expected final_pos in the effective tolerance window [0.05, 0.099], got {final_pos}"
    );
}

#[tokio::test]
async fn stale_telemetry_hold_then_fresh_run_completes() {
    let (state, motor) = state_with_real_can_stub();
    let role = motor.common.role.clone();
    let stale_ms = chrono::Utc::now().timestamp_millis() - 60_000;
    {
        let mut latest = state.latest.write().unwrap();
        latest.insert(
            role.clone(),
            MotorFeedback {
                t_ms: stale_ms,
                role: role.clone(),
                can_id: 1,
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
    let state2 = state.clone();
    let role2 = role.clone();
    let updater = tokio::spawn(async move {
        let mut phase2 = false;
        let t0 = tokio::time::Instant::now();
        loop {
            tokio::time::sleep(Duration::from_millis(3)).await;
            if t0.elapsed() > Duration::from_millis(50) {
                phase2 = true;
            }
            let mut w = state2.latest.write().unwrap();
            let fb = w.get_mut(&role2).unwrap();
            if phase2 {
                fb.t_ms = chrono::Utc::now().timestamp_millis();
                // Cap at the home target so post-home hold verification (500 ms later)
                // still sees the joint on the commissioned pose, not an overshoot past it.
                fb.mech_pos_rad = (fb.mech_pos_rad + 0.03).min(0.12);
            }
        }
    });
    let r = run_with_tracking_budget(state.clone(), motor, 0.0, 0.12, 0.05).await;
    updater.abort();
    assert!(r.is_ok(), "expected Ok, got {r:?}");
}

#[tokio::test]
async fn velocity_gate_blocks_dwell_when_motor_still_coasting() {
    // Pin fix for the handoff-coast-then-stiction-trap failure mode
    // that produced the 0.8-1.2° run-to-run auto-home offset on a
    // gravity-neutral single-actuator rig:
    //
    //   Pre-fix, the dwell gate was position-only. The instant the
    //   measured position crossed into the `effective_tolerance_rad`
    //   window the success counter advanced, so after
    //   `target_dwell_ticks` fresh samples the homer exited Ok even if
    //   the motor was still moving several rad/s. Inside
    //   `Cmd::SetMitHold` the driver then runs
    //   `cmd_stop → write RUN_MODE=0 → cmd_enable → MIT frame`, a
    //   sequence required by the RS03 manual (Ch.2 item 2 / §4.3) that
    //   leaves the actuator unpowered for ~20-50 ms. Residual velocity ×
    //   that disabled window = 2-15 mrad of uncontrolled coast, after
    //   which static friction traps the motor wherever it lands and the
    //   spring-damper hold reports that offset as "home".
    //
    //   Post-fix the dwell success predicate additionally requires
    //   `|mech_vel_rad_s| < target_dwell_max_vel_rad_s`, so the homer
    //   waits for the firmware velocity-command-to-zero to actually
    //   bleed the motor to rest (the firmware is already commanding
    //   vel=0 once inside the position deadband) before handing off.
    //
    // Setup: park the simulated motor at the home target (so the
    // POSITION half of the dwell predicate is always true) but report a
    // velocity of 0.20 rad/s — well above both the fixture's 0.01 rad/s
    // gate and the 0.05 default. The position gate is satisfied every
    // fresh tick but the velocity gate never is, so the homer must time
    // out rather than succeeding. This is the exact property that fails
    // pre-fix: a pure position-only dwell gate would declare Ok within
    // `target_dwell_ticks` fresh samples.
    let gate = 0.01_f32;
    // `home_ramp.rs` clamps `homer_timeout_ms` to a 1 s floor via
    // `.max(1_000)`, so any value below 1000 gives a 1 s effective
    // timeout. That's a reasonable wall-clock for a unit test and still
    // allows hundreds of 5 ms ticks' worth of dwell-gate evaluation
    // before the timeout fires.
    let (state, motor) = state_with_velocity_gate(gate, 1_000);
    let role = motor.common.role.clone();
    let target = 0.10_f32;
    {
        let mut latest = state.latest.write().unwrap();
        latest.insert(
            role.clone(),
            MotorFeedback {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.clone(),
                can_id: 1,
                mech_pos_rad: target,
                mech_vel_rad_s: 0.20,
                torque_nm: 0.0,
                vbus_v: 48.0,
                temp_c: 30.0,
                fault_sta: 0,
                warn_sta: 0,
            },
        );
    }
    let updater = {
        let state = state.clone();
        let role = role.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(2)).await;
                let mut w = state.latest.write().unwrap();
                if let Some(fb) = w.get_mut(&role) {
                    // Refresh timestamp so the sample stays fresh, but
                    // keep position parked at target and velocity above
                    // the gate — the whole point is to isolate the
                    // velocity gate.
                    fb.t_ms = chrono::Utc::now().timestamp_millis();
                    fb.mech_pos_rad = target;
                    fb.mech_vel_rad_s = 0.20;
                }
            }
        })
    };
    let r = run_with_tracking_budget(state.clone(), motor, 0.0, target, 10.0).await;
    updater.abort();
    let Err((reason, _)) = r else {
        panic!(
            "expected Err (velocity gate must block dwell despite in-position telemetry), got {r:?}"
        );
    };
    assert_eq!(
        reason, "timeout",
        "velocity gate should cause timeout, not some other abort (position is in band and tracking budget is loose)"
    );
}

#[tokio::test]
async fn velocity_gate_passes_when_motor_has_settled() {
    // Companion to `velocity_gate_blocks_dwell_when_motor_still_coasting`.
    // Same in-position setup but `mech_vel_rad_s == 0` — proves the gate
    // isn't spuriously blocking the success path when the motor has
    // actually come to rest.
    let gate = 0.05_f32;
    let (state, motor) = state_with_velocity_gate(gate, 5_000);
    let role = motor.common.role.clone();
    let target = 0.10_f32;
    {
        let mut latest = state.latest.write().unwrap();
        latest.insert(
            role.clone(),
            MotorFeedback {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.clone(),
                can_id: 1,
                mech_pos_rad: target,
                mech_vel_rad_s: 0.0,
                torque_nm: 0.0,
                vbus_v: 48.0,
                temp_c: 30.0,
                fault_sta: 0,
                warn_sta: 0,
            },
        );
    }
    let updater = {
        let state = state.clone();
        let role = role.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(2)).await;
                let mut w = state.latest.write().unwrap();
                if let Some(fb) = w.get_mut(&role) {
                    fb.t_ms = chrono::Utc::now().timestamp_millis();
                    fb.mech_pos_rad = target;
                    fb.mech_vel_rad_s = 0.0;
                }
            }
        })
    };
    let r = run_with_tracking_budget(state.clone(), motor, 0.0, target, 10.0).await;
    updater.abort();
    assert!(
        r.is_ok(),
        "expected Ok when velocity is below gate and position is in band, got {r:?}"
    );
}
