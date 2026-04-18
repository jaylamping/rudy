//! Telemetry poller.
//!
//! In Phase 1, the `can::mock` module already drives per-motor feedback into
//! both `AppState::latest` and the `feedback_tx` broadcast channel, so this
//! module just seeds the initial parameter snapshot from the actuator spec.
//! When the real Linux CAN core lands, the periodic type-17 read loop moves
//! here.

use crate::state::SharedState;
use crate::types::{ParamSnapshot, ParamValue};
use std::collections::BTreeMap;

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

    if state.cfg.can.mock {
        return;
    }

    #[cfg(target_os = "linux")]
    if let Some(core) = state.real_can.clone() {
        tokio::spawn(async move {
            if let Err(e) = tokio::task::spawn_blocking({
                let state = state.clone();
                let core = core.clone();
                move || core.refresh_all_params(&state)
            })
            .await
            .expect("refresh_all_params task panicked")
            {
                tracing::warn!(error = ?e, "initial real-CAN snapshot refresh failed");
            }

            let mut tick = tokio::time::interval(std::time::Duration::from_millis(
                state.cfg.telemetry.poll_interval_ms.max(10),
            ));
            loop {
                tick.tick().await;
                if let Err(e) = tokio::task::spawn_blocking({
                    let state = state.clone();
                    let core = core.clone();
                    move || core.poll_once(&state)
                })
                .await
                .expect("real CAN poll task panicked")
                {
                    tracing::warn!(error = ?e, "real-CAN telemetry poll failed");
                }
            }
        });
    }
}
