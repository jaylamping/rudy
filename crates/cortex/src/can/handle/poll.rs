use anyhow::Result;
use chrono::Utc;

use crate::boot_state;
use crate::inventory::Actuator;
use crate::state::SharedState;
use crate::types::{MotorFeedback, ParamValue};

use super::LinuxCanCore;

/// Result of one type-17 sweep across the auxiliary observables.
#[derive(Debug, Clone, Copy)]
struct AuxObservables {
    mech_pos: Option<f32>,
    mech_vel: Option<f32>,
    vbus: Option<f32>,
    fault_sta: Option<u32>,
}

impl LinuxCanCore {
    pub fn poll_once(&self, state: &SharedState) -> Result<()> {
        let motors: Vec<Actuator> = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .actuators()
            .filter(|m| m.common.present)
            .cloned()
            .collect();
        for motor in &motors {
            if !self.backoff.should_poll(&motor.common.role) {
                continue;
            }
            let poll_started_ms = Utc::now().timestamp_millis();
            match self.read_aux_observables(state, motor) {
                Ok(aux) => {
                    self.backoff.record_success(&motor.common.role);
                    self.merge_aux_into_latest(state, motor, poll_started_ms, aux);
                }
                Err(e) => {
                    self.backoff.record_failure(&motor.common.role, &e);
                }
            }
        }
        Ok(())
    }

    fn read_aux_observables(
        &self,
        state: &SharedState,
        motor: &Actuator,
    ) -> Result<AuxObservables> {
        let mech_pos = self.read_named_f32(state, motor, "mech_pos")?;
        let mech_vel = self.read_named_f32(state, motor, "mech_vel")?;
        let vbus = self.read_named_f32(state, motor, "vbus")?;
        let fault_sta = match self.read_named_u32(state, motor, "fault_sta") {
            Ok(v) => v,
            Err(e) => {
                if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                    if io_err.kind() == std::io::ErrorKind::TimedOut {
                        None
                    } else {
                        return Err(e);
                    }
                } else {
                    return Err(e);
                }
            }
        };
        Ok(AuxObservables {
            mech_pos,
            mech_vel,
            vbus,
            fault_sta,
        })
    }

    fn merge_aux_into_latest(
        &self,
        state: &SharedState,
        motor: &Actuator,
        poll_started_ms: i64,
        aux: AuxObservables,
    ) {
        use std::collections::BTreeMap;

        {
            let mut params = state.params.write().expect("params poisoned");
            let snapshot = params.entry(motor.common.role.clone()).or_insert_with(|| {
                crate::types::ParamSnapshot {
                    role: motor.common.role.clone(),
                    values: BTreeMap::new(),
                }
            });
            let spec = state.spec_for(motor.robstride_model());
            for (name, desc) in spec.observables.iter() {
                let value = match name.as_str() {
                    "mech_pos" => match aux.mech_pos {
                        Some(v) => serde_json::json!(v),
                        None => continue,
                    },
                    "mech_vel" => match aux.mech_vel {
                        Some(v) => serde_json::json!(v),
                        None => continue,
                    },
                    "vbus" => match aux.vbus {
                        Some(v) => serde_json::json!(v),
                        None => continue,
                    },
                    "fault_sta" => match aux.fault_sta {
                        Some(v) => serde_json::json!(v),
                        None => continue,
                    },
                    _ => continue,
                };
                snapshot.values.insert(
                    name.clone(),
                    ParamValue {
                        name: name.clone(),
                        index: desc.index,
                        ty: desc.ty.clone(),
                        units: desc.units.clone(),
                        value,
                        hardware_range: desc.hardware_range,
                        // Always `false` here: this loop iterates
                        // `spec.observables` exclusively (mech_pos,
                        // mech_vel, vbus, fault_sta), which the
                        // PUT-param handler refuses to write to.
                        writable: false,
                        desired: None,
                        drift: None,
                    },
                );
            }
        }

        #[derive(Clone, Copy)]
        enum MergeOutcome {
            Type2Won { row_t_ms: i64 },
            Type17Stamped,
            Seeded,
        }

        let (merged, outcome): (MotorFeedback, MergeOutcome) = {
            let mut latest = state.latest.write().expect("latest poisoned");
            let now_ms = Utc::now().timestamp_millis();
            match latest.get_mut(&motor.common.role) {
                Some(row) => {
                    if let Some(v) = aux.vbus {
                        row.vbus_v = v;
                    }
                    if let Some(f) = aux.fault_sta {
                        row.fault_sta = f;
                    }
                    let outcome = if row.t_ms < poll_started_ms {
                        if let Some(p) = aux.mech_pos {
                            row.mech_pos_rad = p;
                        }
                        if let Some(v) = aux.mech_vel {
                            row.mech_vel_rad_s = v;
                        }
                        row.t_ms = now_ms;
                        MergeOutcome::Type17Stamped
                    } else {
                        MergeOutcome::Type2Won { row_t_ms: row.t_ms }
                    };
                    (row.clone(), outcome)
                }
                None => {
                    let seeded = MotorFeedback {
                        t_ms: now_ms,
                        role: motor.common.role.clone(),
                        can_id: motor.common.can_id,
                        mech_pos_rad: aux.mech_pos.unwrap_or_default(),
                        mech_vel_rad_s: aux.mech_vel.unwrap_or_default(),
                        torque_nm: 0.0,
                        vbus_v: aux.vbus.unwrap_or_default(),
                        temp_c: 0.0,
                        fault_sta: aux.fault_sta.unwrap_or_default(),
                        warn_sta: 0,
                    };
                    latest.insert(motor.common.role.clone(), seeded.clone());
                    (seeded, MergeOutcome::Seeded)
                }
            }
        };

        match outcome {
            MergeOutcome::Type2Won { row_t_ms } => tracing::trace!(
                role = %motor.common.role,
                can_id = motor.common.can_id,
                outcome = "type2_won",
                row_t_ms = row_t_ms,
                poll_started_ms = poll_started_ms,
                aux_pos = ?aux.mech_pos,
                aux_vel = ?aux.mech_vel,
                aux_vbus = ?aux.vbus,
                aux_fault = ?aux.fault_sta,
                "aux merge"
            ),
            MergeOutcome::Type17Stamped => tracing::trace!(
                role = %motor.common.role,
                can_id = motor.common.can_id,
                outcome = "type17_stamped",
                poll_started_ms = poll_started_ms,
                aux_pos = ?aux.mech_pos,
                aux_vel = ?aux.mech_vel,
                aux_vbus = ?aux.vbus,
                aux_fault = ?aux.fault_sta,
                "aux merge: type-17 fallback refreshed t_ms (no type-2 this tick)"
            ),
            MergeOutcome::Seeded => tracing::info!(
                role = %motor.common.role,
                can_id = motor.common.can_id,
                "aux merge: seeded latest row from type-17 (first telemetry)"
            ),
        }

        let classify_outcome = boot_state::classify(state, &motor.common.role, merged.mech_pos_rad);
        let aux_seeded_first_row = matches!(outcome, MergeOutcome::Seeded);
        crate::boot_orchestrator::spawn_if_orchestrator_qualifies(
            state.clone(),
            motor.common.role.clone(),
            classify_outcome,
            aux_seeded_first_row,
        );

        let _ = state.feedback_tx.send(merged);
    }
}
