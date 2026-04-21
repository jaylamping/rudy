use super::*;

fn limits(min: f32, max: f32) -> TravelLimits {
    TravelLimits {
        min_rad: min,
        max_rad: max,
        updated_at: None,
    }
}

#[test]
fn distance_to_band_zero_when_in_band() {
    let l = limits(-1.0, 1.0);
    assert_eq!(distance_to_band(0.0, &l), 0.0);
    assert_eq!(distance_to_band(-1.0, &l), 0.0);
    assert_eq!(distance_to_band(1.0, &l), 0.0);
}

#[test]
fn distance_to_band_picks_nearer_edge() {
    let l = limits(-1.0, 1.0);
    assert!((distance_to_band(1.5, &l) - 0.5).abs() < 1e-5);
    assert!((distance_to_band(-1.5, &l) - 0.5).abs() < 1e-5);
}

#[test]
fn recovery_target_lands_inside_band() {
    let l = limits(-1.0, 1.0);
    let t = recovery_target(1.5, &l, 0.1).expect("out of band on max side");
    assert!((t - 0.9).abs() < 1e-5, "got {t}");
    let t = recovery_target(-1.5, &l, 0.1).expect("out of band on min side");
    assert!((t - (-0.9)).abs() < 1e-5, "got {t}");
}

#[test]
fn recovery_target_none_when_in_band() {
    let l = limits(-1.0, 1.0);
    assert!(recovery_target(0.0, &l, 0.1).is_none());
}

#[test]
fn boot_state_permits_enable_only_when_homed() {
    assert!(BootState::Homed.permits_enable());
    assert!(!BootState::InBand.permits_enable());
    assert!(!BootState::Unknown.permits_enable());
    assert!(!BootState::OutOfBand {
        mech_pos_rad: 0.0,
        min_rad: 0.0,
        max_rad: 0.0,
    }
    .permits_enable());
}

/// New boot-orchestrator variants must NOT permit enable. Class-1
/// shenanigans (OffsetChanged) refuse motion until recovery; the
/// orchestrator's own AutoHoming flow refuses operator commands
/// while it's running; HomeFailed requires operator investigation
/// before any motion is allowed.
#[test]
fn new_boot_state_variants_refuse_enable() {
    let oc = BootState::OffsetChanged {
        stored_rad: 0.0,
        current_rad: 0.5,
    };
    let ah = BootState::AutoHoming {
        from_rad: 0.0,
        target_rad: 0.0,
        progress_rad: 0.0,
    };
    let hf = BootState::HomeFailed {
        reason: "tracking_error".into(),
        last_pos_rad: 0.42,
    };
    assert!(!oc.permits_enable());
    assert!(!ah.permits_enable());
    assert!(!hf.permits_enable());
}
