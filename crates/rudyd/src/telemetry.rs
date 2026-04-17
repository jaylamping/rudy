//! Telemetry poller.
//!
//! In Phase 1, the `can::mock` module already drives per-motor feedback into
//! both `AppState::latest` and the `feedback_tx` broadcast channel, so this
//! module just seeds the initial parameter snapshot from the actuator spec.
//! When the real Linux CAN core lands, the periodic type-17 read loop moves
//! here.

use std::collections::BTreeMap;

use crate::state::SharedState;
use crate::types::{ParamSnapshot, ParamValue};

pub fn spawn(state: SharedState) {
    // Seed a placeholder parameter snapshot for every inventoried motor so
    // the UI has something to render before the first real read completes.
    let mut seeded = BTreeMap::new();
    for motor in &state.inventory.motors {
        let mut values = BTreeMap::new();
        for (name, desc) in state.spec.catalog() {
            let default = match desc.ty.as_str() {
                "float" | "f32" | "f64" => serde_json::json!(0.0f32),
                "uint8" | "u8" | "uint16" | "u16" | "uint32" | "u32" => serde_json::json!(0u32),
                _ => serde_json::Value::Null,
            };
            values.insert(
                name.clone(),
                ParamValue {
                    name: name.clone(),
                    index: desc.index,
                    ty: desc.ty.clone(),
                    units: desc.units.clone(),
                    value: default,
                    hardware_range: desc.hardware_range,
                },
            );
        }
        seeded.insert(
            motor.role.clone(),
            ParamSnapshot {
                role: motor.role.clone(),
                values,
            },
        );
    }
    *state.params.write().expect("params poisoned") = seeded;
}
