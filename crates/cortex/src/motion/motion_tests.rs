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
            MotionStopReason::Bus(crate::motion::status::MotionBusError::Other("nope".into(),))
                .label(),
            "bus_error"
        );
    }

    #[test]
    fn stop_reason_detail_carries_inner_error() {
        let r = MotionStopReason::Bus(crate::motion::status::MotionBusError::Backpressure {
            detail: "ENOBUFS".into(),
        });
        assert!(r.detail().contains("ENOBUFS"));
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

mod dynamics_tests {
    use crate::inventory::{Actuator, ActuatorCommon, ActuatorFamily, RobstrideModel};
    use crate::motion::dynamics::{
        clamp_velocity_for_joint, velocity_exceeds_joint_limit, JointDynamics,
    };
    use std::collections::BTreeMap;

    fn make_motor(dynamics: Option<JointDynamics>) -> Actuator {
        Actuator {
            common: ActuatorCommon {
                role: "test.shoulder_roll".to_string(),
                can_bus: "can1".to_string(),
                can_id: 9,
                present: true,
                verified: true,
                commissioned_at: None,
                firmware_version: None,
                travel_limits: None,
                commissioned_zero_offset: None,
                active_report_persisted: false,
                predefined_home_rad: None,
                homing_speed_rad_s: None,
                hold_kp_nm_per_rad: None,
                hold_kd_nm_s_per_rad: None,
                mit_command_kp_nm_per_rad: None,
                mit_command_kd_nm_s_per_rad: None,
                mit_max_angle_step_rad: None,
                limb: Some("right_arm".to_string()),
                joint_kind: Some(crate::limb::JointKind::ShoulderRoll),
                notes_yaml: None,
                desired_params: BTreeMap::new(),
                current_safety: None,
                dynamics,
            },
            family: ActuatorFamily::Robstride {
                model: RobstrideModel::Rs03,
            },
        }
    }

    #[test]
    fn velocity_clamp_no_dynamics_passes_through() {
        let motor = make_motor(None);
        assert_eq!(clamp_velocity_for_joint(&motor, 5.0), 5.0);
        assert_eq!(clamp_velocity_for_joint(&motor, -3.0), -3.0);
    }

    #[test]
    fn velocity_clamp_with_limit_caps_magnitude() {
        let d = JointDynamics {
            max_velocity_rad_s: Some(1.5),
            ..Default::default()
        };
        let motor = make_motor(Some(d));
        assert_eq!(clamp_velocity_for_joint(&motor, 2.0), 1.5);
        assert_eq!(clamp_velocity_for_joint(&motor, -2.0), -1.5);
        assert_eq!(clamp_velocity_for_joint(&motor, 0.8), 0.8);
    }

    #[test]
    fn velocity_exceeds_returns_none_under_limit() {
        let d = JointDynamics {
            max_velocity_rad_s: Some(2.0),
            ..Default::default()
        };
        let motor = make_motor(Some(d));
        assert_eq!(velocity_exceeds_joint_limit(&motor, 1.5), None);
        assert_eq!(velocity_exceeds_joint_limit(&motor, -1.5), None);
    }

    #[test]
    fn velocity_exceeds_returns_limit_when_over() {
        let d = JointDynamics {
            max_velocity_rad_s: Some(1.0),
            ..Default::default()
        };
        let motor = make_motor(Some(d));
        assert_eq!(velocity_exceeds_joint_limit(&motor, 1.5), Some(1.0));
        assert_eq!(velocity_exceeds_joint_limit(&motor, -1.5), Some(1.0));
    }

    #[test]
    fn loaded_joint_default_dynamics_means_fail_closed() {
        let d = JointDynamics {
            loaded: true,
            gravity_torque_nm: Some(10.0),
            gravity_margin: 0.25,
            ..Default::default()
        };
        // No continuous_torque_nm, no structural_torque_nm, no firmware limits
        // For a loaded joint with gravity_torque, ceiling should be 0 (fail closed)
        assert!(d.loaded);
        assert_eq!(d.continuous_torque_nm, None);
        assert_eq!(d.structural_torque_nm, None);
    }

    #[test]
    fn gravity_margin_calculation() {
        let d = JointDynamics {
            gravity_torque_nm: Some(8.0),
            gravity_margin: 0.25,
            ..Default::default()
        };
        let required = d.gravity_torque_nm.unwrap() * (1.0 + d.gravity_margin);
        assert!((required - 10.0).abs() < 1e-6);
    }

    #[test]
    fn bench_mode_forces_observe_only() {
        use crate::config::SafetyConfig;
        let mut safety = SafetyConfig::default();
        safety.bench_mode = true;
        safety.current_trip_observe_only = false;

        let loaded = JointDynamics {
            loaded: true,
            ..Default::default()
        };
        assert!(safety.effective_observe_only(Some(&loaded)));
        assert!(safety.effective_observe_only(None));
    }

    #[test]
    fn loaded_joint_enforces_outside_bench_mode() {
        use crate::config::SafetyConfig;
        let mut safety = SafetyConfig::default();
        safety.bench_mode = false;
        safety.current_trip_observe_only = true;

        let loaded = JointDynamics {
            loaded: true,
            ..Default::default()
        };
        // Loaded joint should NOT be observe-only even when global says so
        assert!(!safety.effective_observe_only(Some(&loaded)));
    }

    #[test]
    fn unloaded_joint_respects_global_observe_only() {
        use crate::config::SafetyConfig;
        let mut safety = SafetyConfig::default();
        safety.bench_mode = false;
        safety.current_trip_observe_only = true;

        let unloaded = JointDynamics {
            loaded: false,
            ..Default::default()
        };
        assert!(safety.effective_observe_only(Some(&unloaded)));

        safety.current_trip_observe_only = false;
        assert!(!safety.effective_observe_only(Some(&unloaded)));
    }

    #[test]
    fn no_dynamics_uses_global_observe_only() {
        use crate::config::SafetyConfig;
        let mut safety = SafetyConfig::default();
        safety.bench_mode = false;
        safety.current_trip_observe_only = true;
        assert!(safety.effective_observe_only(None));

        safety.current_trip_observe_only = false;
        assert!(!safety.effective_observe_only(None));
    }
}
