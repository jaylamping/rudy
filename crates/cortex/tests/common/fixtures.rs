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
    role: shoulder_actuator_a
    can_bus: can1
    can_id: 0x08
    firmware_version: "1.2.3"
    verified: true
    present: true
    family:
      kind: robstride
      model: rs03
  - kind: actuator
    role: shoulder_actuator_b
    can_bus: can1
    can_id: 0x09
    firmware_version: "1.2.3"
    verified: false
    present: true
    family:
      kind: robstride
      model: rs03
"#;
