// Mirror of rudyd's `types::MotorFeedback`.
// Source of truth: crates/rudyd/src/types.rs
// Phase-2 TODO: replace by ts-rs auto-generation; until then, keep in sync
// by hand. The server serializes this struct both as JSON (REST) and CBOR
// (WebTransport datagrams).
export interface MotorFeedback {
  t_ms: number;
  role: string;
  can_id: number;
  mech_pos_rad: number;
  mech_vel_rad_s: number;
  torque_nm: number;
  vbus_v: number;
  temp_c: number;
  fault_sta: number;
  warn_sta: number;
}
