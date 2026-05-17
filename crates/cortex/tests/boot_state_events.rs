//! Boot-state classifier events that keep SPA badges fresh.

use cortex::boot_state::{self, BootState, ClassifyOutcome};
use cortex::can::angle::UnwrappedAngle;
use cortex::inventory::TravelLimits;
use cortex::types::SafetyEvent;

#[path = "common/mod.rs"]
mod common;

const ROLE: &str = "shoulder_actuator_a";

#[test]
fn classify_out_of_band_to_in_band_broadcasts_boot_state_changed() {
    let (state, _dir) = common::make_state();
    {
        let mut inv = state.inventory.write().expect("inventory");
        let a = common::actuator_mut(&mut inv, ROLE).expect("actuator");
        a.common.travel_limits = Some(TravelLimits {
            min_rad: -1.0,
            max_rad: 1.0,
            updated_at: None,
        });
    }
    common::set_boot_state(
        &state,
        ROLE,
        BootState::OutOfBand {
            mech_pos_rad: 2.0,
            min_rad: -1.0,
            max_rad: 1.0,
        },
    );
    let mut rx = state.safety_event_tx.subscribe();

    let outcome = boot_state::classify(&state, ROLE, UnwrappedAngle::new(0.0));

    assert!(matches!(
        outcome,
        ClassifyOutcome::Changed {
            prev: BootState::OutOfBand { .. },
            new: BootState::InBand,
        }
    ));
    let event = rx.try_recv().expect("boot_state_changed event");
    assert!(matches!(
        event,
        SafetyEvent::BootStateChanged {
            role,
            from,
            to,
            ..
        } if role == ROLE && from == "out_of_band" && to == "in_band"
    ));
}
