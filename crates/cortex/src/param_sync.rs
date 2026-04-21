//! Desired-vs-live param decoration for writable firmware limits.

use crate::hardware::spec::ActuatorSpec;
use crate::inventory::Actuator;
use crate::types::{ParamDrift, ParamSnapshot};

/// Compare JSON param values; uses approximate float equality.
pub fn json_values_close(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a, b) {
        (serde_json::Value::Number(x), serde_json::Value::Number(y)) => {
            if let (Some(xf), Some(yf)) = (x.as_f64(), y.as_f64()) {
                (xf - yf).abs() < 1e-5
            } else {
                x == y
            }
        }
        _ => a == b,
    }
}

/// Fills `desired` / `drift` on each catalog entry. Returns the number of drifted *writable*
/// params (where inventory has a desired value that differs from live).
pub fn decorate_snapshot(motor: &Actuator, spec: &ActuatorSpec, snap: &mut ParamSnapshot) -> u32 {
    let mut drifted = 0u32;

    for (name, _desc, _writable) in spec.catalog() {
        let Some(pv) = snap.values.get_mut(&name) else {
            continue;
        };

        if spec.firmware_limits.contains_key(&name) {
            pv.desired = motor.common.desired_params.get(&name).cloned();
            pv.drift = match &pv.desired {
                Some(desired) if !json_values_close(&pv.value, desired) => {
                    drifted += 1;
                    Some(ParamDrift {
                        live: pv.value.clone(),
                        desired: desired.clone(),
                    })
                }
                _ => None,
            };
        } else {
            pv.desired = None;
            pv.drift = None;
        }
    }

    drifted
}
