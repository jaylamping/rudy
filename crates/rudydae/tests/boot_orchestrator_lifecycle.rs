//! Contract tests for `boot_orchestrator::maybe_run` (mock CAN).
//!
//! Pins the commissioning / auto-home state machine: tolerance vs stored offset,
//! telemetry freshness, travel band, idempotency, and the `auto_home_on_boot`
//! master switch.

mod common;

use std::time::Duration;

use rudydae::boot_orchestrator;
use rudydae::boot_state::{BootState, ClassifyOutcome};
use rudydae::types::MotorFeedback;

const ROLE: &str = "shoulder_actuator_a";

fn set_commissioned_zero(state: &rudydae::state::SharedState, role: &str, rad: f32) {
    let mut inv = state.inventory.write().expect("inventory poisoned");
    let a = common::actuator_mut(&mut inv, role).expect("fixture motor");
    a.common.commissioned_zero_offset = Some(rad);
}

#[tokio::test]
async fn orchestrator_skips_when_auto_home_disabled() {
    let (state, _dir) = common::make_state_auto_home_on_boot(false);
    set_commissioned_zero(&state, ROLE, 0.0);
    common::seed_feedback(&state);
    common::set_boot_state(&state, ROLE, BootState::InBand);

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;

    assert!(matches!(
        rudydae::boot_state::current(&state, ROLE),
        BootState::InBand
    ));
}

#[tokio::test]
async fn orchestrator_returns_while_uncommissioned_and_stays_idempotent() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(&state, ROLE, BootState::InBand);

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;
    assert!(matches!(
        rudydae::boot_state::current(&state, ROLE),
        BootState::InBand
    ));
    assert!(
        state
            .boot_orchestrator_attempted
            .lock()
            .expect("poisoned")
            .contains(ROLE),
        "first run should record the role so we do not spin until commission"
    );

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;
    assert!(matches!(
        rudydae::boot_state::current(&state, ROLE),
        BootState::InBand
    ));
}

#[tokio::test]
async fn orchestrator_mismatch_sets_offset_changed() {
    let (state, _dir) = common::make_state();
    set_commissioned_zero(&state, ROLE, 0.5);
    common::seed_feedback(&state);
    common::set_boot_state(&state, ROLE, BootState::InBand);

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;

    let bs = rudydae::boot_state::current(&state, ROLE);
    let BootState::OffsetChanged {
        stored_rad,
        current_rad,
    } = bs
    else {
        panic!("expected OffsetChanged, got {bs:?}");
    };
    assert!((stored_rad - 0.5).abs() < 1e-5);
    assert!((current_rad - 0.0).abs() < 1e-5);
}

#[tokio::test]
async fn orchestrator_happy_path_auto_homes_on_mock_can() {
    let (state, _dir) = common::make_state();
    set_commissioned_zero(&state, ROLE, 0.0);
    common::seed_feedback(&state);
    common::set_boot_state(&state, ROLE, BootState::InBand);

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;

    assert!(matches!(
        rudydae::boot_state::current(&state, ROLE),
        BootState::Homed
    ));
}

#[tokio::test]
async fn orchestrator_second_invocation_is_noop_after_success() {
    let (state, _dir) = common::make_state();
    set_commissioned_zero(&state, ROLE, 0.0);
    common::seed_feedback(&state);
    common::set_boot_state(&state, ROLE, BootState::InBand);

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;
    assert!(matches!(
        rudydae::boot_state::current(&state, ROLE),
        BootState::Homed
    ));

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;
    assert!(matches!(
        rudydae::boot_state::current(&state, ROLE),
        BootState::Homed
    ));
}

#[tokio::test]
async fn orchestrator_waits_for_fresh_telemetry_then_succeeds() {
    let (state, _dir) = common::make_state();
    set_commissioned_zero(&state, ROLE, 0.0);
    common::seed_feedback(&state);
    common::set_boot_state(&state, ROLE, BootState::InBand);

    {
        let mut latest = state.latest.write().expect("latest poisoned");
        let fb = latest.get_mut(ROLE).expect("seeded feedback");
        fb.t_ms = chrono::Utc::now().timestamp_millis() - 10_000;
    }

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;
    assert!(!matches!(
        rudydae::boot_state::current(&state, ROLE),
        BootState::Homed
    ));

    common::seed_feedback(&state);
    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;
    assert!(matches!(
        rudydae::boot_state::current(&state, ROLE),
        BootState::Homed
    ));
}

#[tokio::test]
async fn orchestrator_skips_when_mech_pos_outside_travel_limits() {
    let (state, _dir) = common::make_state();
    set_commissioned_zero(&state, ROLE, 0.0);
    common::set_travel_limits(&state, ROLE, -0.05, 0.05);
    common::seed_feedback(&state);
    common::set_boot_state(&state, ROLE, BootState::InBand);

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;

    assert!(matches!(
        rudydae::boot_state::current(&state, ROLE),
        BootState::InBand
    ));
    assert!(
        !state
            .boot_orchestrator_attempted
            .lock()
            .expect("poisoned")
            .contains(ROLE),
        "OutOfBand-style skip should clear attempted so a future InBand re-entry can retry"
    );

    // Nudge telemetry into band; orchestrator should be able to run again.
    {
        let mut latest = state.latest.write().expect("latest poisoned");
        latest.insert(
            ROLE.into(),
            MotorFeedback {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: ROLE.into(),
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

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;
    assert!(matches!(
        rudydae::boot_state::current(&state, ROLE),
        BootState::Homed
    ));
}

#[tokio::test]
async fn orchestrator_homer_timeout_marks_home_failed() {
    let (state, _dir) = common::make_state_homer_times_out_quickly();
    set_commissioned_zero(&state, ROLE, 0.0);
    common::seed_feedback(&state);
    common::set_boot_state(&state, ROLE, BootState::InBand);

    boot_orchestrator::maybe_run(state.clone(), ROLE.into()).await;

    let bs = rudydae::boot_state::current(&state, ROLE);
    let BootState::HomeFailed { reason, .. } = bs else {
        panic!("expected HomeFailed, got {bs:?}");
    };
    assert!(
        reason.contains("timeout"),
        "expected timeout abort, got reason={reason:?}"
    );

    let audit_path = state.cfg.paths.audit_log.clone();
    let raw = std::fs::read_to_string(audit_path).expect("audit log");
    assert!(
        raw.lines()
            .any(|l| l.contains("boot_orchestrator_home_failed")),
        "audit log should record boot_orchestrator_home_failed"
    );
}

#[tokio::test]
async fn spawn_if_triggers_maybe_run_on_out_of_band_to_in_band() {
    let (state, _dir) = common::make_state();
    set_commissioned_zero(&state, ROLE, 0.0);
    common::seed_feedback(&state);
    common::set_boot_state(&state, ROLE, BootState::InBand);

    let outcome = ClassifyOutcome::Changed {
        prev: BootState::OutOfBand {
            mech_pos_rad: 2.0,
            min_rad: -1.0,
            max_rad: 1.0,
        },
        new: BootState::InBand,
    };

    boot_orchestrator::spawn_if_orchestrator_qualifies(state.clone(), ROLE.into(), outcome, false);

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if matches!(rudydae::boot_state::current(&state, ROLE), BootState::Homed) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("orchestrator should reach Homed within 3s");
}
