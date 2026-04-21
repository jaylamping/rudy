//! Consolidated motion unit tests (see `motion/mod.rs`).

mod intent_tests {
    use crate::motion::intent::{
        default_turnaround_rad, MotionIntent, OVERSHOOT_S, SWEEP_BASE_INSET_RAD,
        WAVE_BASE_INSET_RAD,
    };

    #[test]
    fn intent_kind_str_matches_serde_tag() {
        let cases = [
            (
                MotionIntent::Sweep {
                    speed_rad_s: 0.1,
                    turnaround_rad: 0.05,
                },
                "sweep",
            ),
            (
                MotionIntent::Wave {
                    center_rad: 0.0,
                    amplitude_rad: 0.5,
                    speed_rad_s: 0.1,
                    turnaround_rad: 0.02,
                },
                "wave",
            ),
            (MotionIntent::Jog { vel_rad_s: 0.1 }, "jog"),
        ];
        for (intent, expected) in cases {
            assert_eq!(intent.kind_str(), expected);
            let json = serde_json::to_value(&intent).unwrap();
            assert_eq!(json["kind"], expected);
        }
    }

    #[test]
    fn default_turnaround_scales_with_speed() {
        let sweep = MotionIntent::Sweep {
            speed_rad_s: 0.0,
            turnaround_rad: 0.0,
        };
        let zero = default_turnaround_rad(&sweep, 0.0);
        assert!((zero - SWEEP_BASE_INSET_RAD).abs() < 1e-6);
        let mid = default_turnaround_rad(&sweep, 0.5);
        assert!((mid - (SWEEP_BASE_INSET_RAD + 0.5 * OVERSHOOT_S)).abs() < 1e-6);
        let fast = default_turnaround_rad(&sweep, 2.0);
        assert!((fast - (SWEEP_BASE_INSET_RAD + 2.0 * OVERSHOOT_S)).abs() < 1e-6);
    }

    #[test]
    fn default_turnaround_uses_per_pattern_base() {
        let sweep = default_turnaround_rad(
            &MotionIntent::Sweep {
                speed_rad_s: 0.0,
                turnaround_rad: 0.0,
            },
            0.0,
        );
        let wave = default_turnaround_rad(
            &MotionIntent::Wave {
                center_rad: 0.0,
                amplitude_rad: 0.0,
                speed_rad_s: 0.0,
                turnaround_rad: 0.0,
            },
            0.0,
        );
        assert!(sweep > wave);
        assert!((sweep - SWEEP_BASE_INSET_RAD).abs() < 1e-6);
        assert!((wave - WAVE_BASE_INSET_RAD).abs() < 1e-6);
    }

    #[test]
    fn default_turnaround_is_always_zero_for_jog() {
        let v = default_turnaround_rad(&MotionIntent::Jog { vel_rad_s: 0.5 }, 0.5);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn default_turnaround_treats_negative_speed_as_magnitude() {
        let sweep = MotionIntent::Sweep {
            speed_rad_s: 0.0,
            turnaround_rad: 0.0,
        };
        let pos = default_turnaround_rad(&sweep, 0.5);
        let neg = default_turnaround_rad(&sweep, -0.5);
        assert!((pos - neg).abs() < 1e-6);
        assert!(neg > 0.0);
    }
}

mod status_tests {
    use crate::motion::status::{MotionState, MotionStopReason};

    #[test]
    fn motion_state_serializes_snake_case() {
        let s = serde_json::to_string(&MotionState::Running).unwrap();
        assert_eq!(s, r#""running""#);
        let s = serde_json::to_string(&MotionState::Stopped).unwrap();
        assert_eq!(s, r#""stopped""#);
    }

    #[test]
    fn stop_reason_label_matches_audit_contract() {
        assert_eq!(MotionStopReason::Operator.label(), "operator");
        assert_eq!(
            MotionStopReason::HeartbeatLapsed.label(),
            "heartbeat_lapsed"
        );
        assert_eq!(MotionStopReason::Superseded.label(), "superseded");
        assert_eq!(
            MotionStopReason::BusError("nope".into()).label(),
            "bus_error"
        );
    }

    #[test]
    fn stop_reason_detail_carries_inner_error() {
        let r = MotionStopReason::BusError("ENOBUFS".into());
        assert_eq!(r.detail(), "ENOBUFS");
        let r = MotionStopReason::Operator;
        assert_eq!(r.detail(), "operator");
    }
}

mod sweep_tests {
    use crate::inventory::TravelLimits;
    use crate::motion::patterns::sweep::{step, SweepState};

    fn limits(min: f32, max: f32) -> TravelLimits {
        TravelLimits {
            min_rad: min,
            max_rad: max,
            updated_at: None,
        }
    }

    #[test]
    fn initial_direction_from_band_midpoint() {
        let l = limits(-1.0, 1.0);
        assert_eq!(SweepState::from_position(-0.5, &l).direction, 1.0);
        assert_eq!(SweepState::from_position(0.5, &l).direction, -1.0);
        assert_eq!(SweepState::from_position(0.0, &l).direction, 1.0);
    }

    #[test]
    fn step_flips_direction_at_inset() {
        let l = limits(-1.0, 1.0);
        let s = SweepState { direction: 1.0 };
        let (v, ns) = step(0.5, s, &l, 0.1, 0.05);
        assert!(v > 0.0);
        assert_eq!(ns.direction, 1.0);
        let (v, ns) = step(0.96, s, &l, 0.1, 0.05);
        assert!(v < 0.0);
        assert_eq!(ns.direction, -1.0);
    }

    #[test]
    fn step_flips_direction_at_lower_inset() {
        let l = limits(-1.0, 1.0);
        let s = SweepState { direction: -1.0 };
        let (v, ns) = step(-0.96, s, &l, 0.1, 0.05);
        assert!(v > 0.0);
        assert_eq!(ns.direction, 1.0);
    }

    #[test]
    fn step_speed_magnitude_is_caller_supplied() {
        let l = limits(-1.0, 1.0);
        let s = SweepState { direction: 1.0 };
        let (v, _) = step(0.0, s, &l, 0.42, 0.05);
        assert!((v.abs() - 0.42).abs() < 1e-6);
    }

    #[test]
    fn step_collapsed_band_returns_zero_velocity() {
        let l = limits(-0.05, 0.05);
        let s = SweepState { direction: 1.0 };
        let (v, _) = step(0.0, s, &l, 0.1, 0.5);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn step_negative_speed_treated_as_magnitude() {
        let l = limits(-1.0, 1.0);
        let s = SweepState { direction: 1.0 };
        let (v, _) = step(0.0, s, &l, -0.3, 0.05);
        assert!(v > 0.0);
        assert!((v - 0.3).abs() < 1e-6);
    }
}

mod wave_tests {
    use crate::inventory::TravelLimits;
    use crate::motion::patterns::wave::{step, WaveState};

    fn limits(min: f32, max: f32) -> TravelLimits {
        TravelLimits {
            min_rad: min,
            max_rad: max,
            updated_at: None,
        }
    }

    #[test]
    fn wave_oscillates_around_center() {
        let l = limits(-1.0, 1.0);
        let s = WaveState { direction: 1.0 };
        let (v, ns) = step(0.0, s, &l, 0.0, 0.5, 0.1, 0.0);
        assert!(v > 0.0);
        assert_eq!(ns.direction, 1.0);
        let (v, ns) = step(0.55, s, &l, 0.0, 0.5, 0.1, 0.0);
        assert!(v < 0.0);
        assert_eq!(ns.direction, -1.0);
    }

    #[test]
    fn wave_clips_to_band() {
        let l = limits(-0.3, 0.3);
        let s = WaveState { direction: 1.0 };
        let (v, ns) = step(0.31, s, &l, 0.0, 1.0, 0.1, 0.0);
        assert!(v < 0.0);
        assert_eq!(ns.direction, -1.0);
    }

    #[test]
    fn wave_zero_amplitude_returns_zero() {
        let l = limits(-1.0, 1.0);
        let s = WaveState { direction: 1.0 };
        let (v, _) = step(0.0, s, &l, 0.0, 0.0, 0.1, 0.0);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn wave_initial_direction_from_center() {
        let s = WaveState::from_position(-0.5, 0.0);
        assert_eq!(s.direction, 1.0);
        let s = WaveState::from_position(0.5, 0.0);
        assert_eq!(s.direction, -1.0);
    }

    #[test]
    fn wave_clips_center_to_band() {
        let l = limits(-1.0, 1.0);
        let s = WaveState { direction: 1.0 };
        let (v, ns) = step(0.6, s, &l, 1.5, 0.5, 0.1, 0.0);
        assert!(v > 0.0);
        let (v, _ns) = step(1.0, ns, &l, 1.5, 0.5, 0.1, 0.0);
        assert!(v < 0.0);
    }
}
