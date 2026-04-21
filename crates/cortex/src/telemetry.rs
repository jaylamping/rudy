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
    let inventory_snap: Vec<crate::inventory::Actuator> = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuators()
        .cloned()
        .collect();
    for motor in &inventory_snap {
        let spec = state.spec_for(motor.robstride_model());
        let mut values = BTreeMap::new();
        for (name, desc) in spec.catalog() {
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
            motor.common.role.clone(),
            ParamSnapshot {
                role: motor.common.role.clone(),
                values,
            },
        );
    }
    *state.params.write().expect("params poisoned") = seeded;

    // Mock CAN drives its own feedback + parameter shadow; only the real
    // Linux CAN core needs the periodic type-17 poller.
    //
    // Per-motor errors are isolated and rate-limited inside
    // `LinuxCanCore` via `can::backoff::MotorBackoff`, so the loop here
    // does not need to handle individual flaky motors. The remaining
    // `warn!` arms are defensive: today `refresh_all_params` and
    // `poll_once` always return `Ok(())`, but if a future refactor makes
    // them fallible at the batch level (e.g. losing the SocketCAN handle
    // entirely) we still want a journal entry rather than a silent loop.
    if !state.cfg.can.mock {
        #[cfg(target_os = "linux")]
        if let Some(core) = state.real_can.clone() {
            tokio::spawn(async move {
                // Layer 4: RAM-write low torque/speed before doing
                // ANYTHING else with the bus. Runs once at startup; the
                // operator-initiated home transition restores per-motor
                // full limits via `home::run_homer`.
                {
                    let state = state.clone();
                    let core = core.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        core.seed_boot_low_limits(&state);
                    })
                    .await;
                }

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
                        tracing::warn!(error = ?e, "real-CAN telemetry poll batch failed");
                    }
                }
            });
        }
    }
}
