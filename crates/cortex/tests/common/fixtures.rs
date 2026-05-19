//! Static YAML blobs shared by [`super::make_state`].
pub const SPEC_YAML: &str = r#"
schema_version: 2
actuator_model: RS03

firmware_limits:
  limit_torque:
    index: 0x700B
    type: float
    units: nm
    hardware_range: [0.0, 60.0]
  limit_spd:
    index: 0x7017
    type: float
    units: rad_per_s
    hardware_range: [0.0, 20.0]
  run_mode:
    index: 0x7005
    type: uint8

observables:
  mech_pos:
    index: 0x7019                       # type-17 shadow of 0x3016
    type: float
    units: rad
  vbus:
    index: 0x701C                       # type-17 shadow of 0x300C
    type: float
    units: volts
"#;

pub const INVENTORY_YAML: &str = r#"
schema_version: 2
devices:
  - kind: actuator
    role: right_arm.shoulder_roll
    can_bus: can1
    can_id: 0x09
    firmware_version: "1.2.3"
    verified: true
    present: true
    limb: right_arm
    joint_kind: shoulder_roll
    stop_behavior: hold
    family:
      kind: robstride
      model: rs03
  - kind: actuator
    role: right_arm.shoulder_pitch
    can_bus: can1
    can_id: 0x08
    firmware_version: "1.2.3"
    verified: false
    present: true
    limb: right_arm
    joint_kind: shoulder_pitch
    family:
      kind: robstride
      model: rs03
"#;
