//! Direct [`cortex::motion::preflight::PreflightChecks`] coverage for
//! `target_position_rad` vs velocity projection.

#[path = "common/mod.rs"]
mod common;

use cortex::boot_state::BootState;
use cortex::motion::preflight::{PreflightChecks, PreflightFailure};
#[test]
fn target_position_trips_boot_max_step_when_not_homed() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);

    let pf = PreflightChecks {
        state: &state,
        role: "shoulder_actuator_a",
        vel_rad_s: 0.0,
        horizon_ms: 50,
        target_position_rad: Some(1.0),
    };
    let err = pf
        .run()
        .expect_err("large target must exceed boot_max_step");
    assert!(
        matches!(err, PreflightFailure::StepTooLarge { .. }),
        "expected StepTooLarge, got {err:?}"
    );
}

#[test]
fn target_position_ignores_velocity_for_geometric_step_cap() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);

    // Without target: absurd velocity × horizon would blow boot_max_step.
    let huge_vel = PreflightChecks {
        state: &state,
        role: "shoulder_actuator_a",
        vel_rad_s: 50.0,
        horizon_ms: 100,
        target_position_rad: None,
    };
    assert!(
        huge_vel.run().is_err(),
        "velocity projection should fail step cap"
    );

    // With target near feedback: step small; velocity ignored for projection.
    let mit_like = PreflightChecks {
        state: &state,
        role: "shoulder_actuator_a",
        vel_rad_s: 50.0,
        horizon_ms: 100,
        target_position_rad: Some(0.12),
    };
    mit_like
        .run()
        .expect("small target should pass despite huge vel_rad_s");
}

#[test]
fn active_fault_blocks_preflight() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);

    {
        let mut latest = state.latest.write().expect("latest");
        let mut row = latest.get("shoulder_actuator_a").expect("seeded").clone();
        row.fault_sta = 1;
        row.warn_sta = 0;
        latest.insert("shoulder_actuator_a".into(), row);
    }

    let pf = PreflightChecks {
        state: &state,
        role: "shoulder_actuator_a",
        vel_rad_s: 0.0,
        horizon_ms: 10,
        target_position_rad: None,
    };
    let err = pf.run().expect_err("fault must refuse motion");
    assert!(
        matches!(err, PreflightFailure::ActiveFault { .. }),
        "{err:?}"
    );
}
