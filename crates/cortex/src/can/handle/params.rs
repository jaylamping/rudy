use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};

use crate::can::worker::WriteValue;
use crate::inventory::Actuator;
use crate::spec::ParamDescriptor;
use crate::state::SharedState;
use crate::types::{ParamSnapshot, ParamValue};

use super::{LinuxCanCore, PARAM_TIMEOUT};

impl LinuxCanCore {
    pub fn write_param(
        &self,
        motor: &Actuator,
        desc: &ParamDescriptor,
        value: &serde_json::Value,
        save_after: bool,
    ) -> Result<serde_json::Value> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        let normalized: serde_json::Value = match desc.ty.as_str() {
            "float" | "f32" | "f64" => {
                let v = value
                    .as_f64()
                    .ok_or_else(|| anyhow!("expected numeric JSON value for {}", desc.ty))?
                    as f32;
                handle.write_param(
                    self.host_id,
                    motor.common.can_id,
                    desc.index,
                    WriteValue::F32(v),
                )?;
                serde_json::json!(v)
            }
            "uint8" | "u8" => {
                let v = value
                    .as_u64()
                    .ok_or_else(|| anyhow!("expected unsigned integer JSON value"))?;
                let v = u8::try_from(v).context("u8 parameter out of range")?;
                handle.write_param(
                    self.host_id,
                    motor.common.can_id,
                    desc.index,
                    WriteValue::U8(v),
                )?;
                serde_json::json!(v)
            }
            "uint16" | "u16" => {
                let v = value
                    .as_u64()
                    .ok_or_else(|| anyhow!("expected unsigned integer JSON value"))?;
                let v = u16::try_from(v).context("u16 parameter out of range")?;
                handle.write_param(
                    self.host_id,
                    motor.common.can_id,
                    desc.index,
                    WriteValue::U32(v as u32),
                )?;
                serde_json::json!(v)
            }
            "uint32" | "u32" => {
                let v = value
                    .as_u64()
                    .ok_or_else(|| anyhow!("expected unsigned integer JSON value"))?;
                let v = u32::try_from(v).context("u32 parameter out of range")?;
                handle.write_param(
                    self.host_id,
                    motor.common.can_id,
                    desc.index,
                    WriteValue::U32(v),
                )?;
                serde_json::json!(v)
            }
            other => bail!("writes for parameter type {other} are not supported"),
        };

        if save_after {
            handle.save_params(self.host_id, motor.common.can_id)?;
        }

        Ok(normalized)
    }

    pub fn refresh_all_params(&self, state: &SharedState) -> Result<()> {
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
            match self.read_full_snapshot(state, motor) {
                Ok((snapshot, drifted)) => {
                    self.backoff.record_success(&motor.common.role);
                    let role = motor.common.role.clone();
                    state
                        .params
                        .write()
                        .expect("params poisoned")
                        .insert(role.clone(), snapshot);
                    state
                        .drift_counts
                        .write()
                        .expect("drift_counts poisoned")
                        .insert(role, drifted);
                }
                Err(e) => {
                    self.backoff.record_failure(&motor.common.role, &e);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn read_full_snapshot(
        &self,
        state: &SharedState,
        motor: &Actuator,
    ) -> Result<(ParamSnapshot, u32)> {
        let spec = state.spec_for(motor.robstride_model());
        let mut values = BTreeMap::new();
        for (name, desc, writable) in spec.catalog() {
            let name_str = name.clone();
            let value = self.read_param_value(motor, &name_str, &desc)?;
            values.insert(
                name_str.clone(),
                ParamValue {
                    name: name_str,
                    index: desc.index,
                    ty: desc.ty.clone(),
                    units: desc.units.clone(),
                    value,
                    hardware_range: desc.hardware_range,
                    writable,
                    desired: None,
                    drift: None,
                },
            );
        }
        let mut snapshot = ParamSnapshot {
            role: motor.common.role.clone(),
            values,
        };
        let drifted = crate::param_sync::decorate_snapshot(motor, &spec, &mut snapshot);
        Ok((snapshot, drifted))
    }

    /// Slow path (~5s): refresh writable params from the bus, reconcile `firmware_version` into
    /// inventory when it changes, recompute drift counts.
    pub fn reconcile_inventory(&self, state: &SharedState) -> Result<()> {
        let inv_path = state.cfg.paths.inventory.clone();
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
            if let Err(e) = self.reconcile_one_motor(state, &inv_path, motor) {
                self.backoff.record_failure(&motor.common.role, &e);
            } else {
                self.backoff.record_success(&motor.common.role);
            }
        }
        Ok(())
    }

    fn reconcile_one_motor(
        &self,
        state: &SharedState,
        inv_path: &Path,
        motor: &Actuator,
    ) -> Result<()> {
        let spec = state.spec_for(motor.robstride_model());

        // Device-owned: live firmware string vs inventory cache
        if let Some(desc) = spec.observables.get("firmware_version") {
            let live = self.read_param_value(motor, "firmware_version", desc)?;
            if let serde_json::Value::String(s) = &live {
                let inv_fw = motor.common.firmware_version.as_deref();
                if inv_fw != Some(s.as_str()) {
                    update_actuator_firmware_version_yaml(
                        state,
                        inv_path,
                        &motor.common.role,
                        s.clone(),
                    )?;
                }
            }
            let motor = state
                .inventory
                .read()
                .expect("inventory poisoned")
                .actuator_by_role(&motor.common.role)
                .cloned()
                .expect("motor must exist");
            {
                let mut params = state.params.write().expect("params poisoned");
                let snap =
                    params
                        .entry(motor.common.role.clone())
                        .or_insert_with(|| ParamSnapshot {
                            role: motor.common.role.clone(),
                            values: BTreeMap::new(),
                        });
                let fw_desc = spec.observables.get("firmware_version").expect("fw");
                snap.values
                    .entry("firmware_version".to_string())
                    .and_modify(|pv| pv.value = live.clone())
                    .or_insert_with(|| ParamValue {
                        name: "firmware_version".into(),
                        index: fw_desc.index,
                        ty: fw_desc.ty.clone(),
                        units: fw_desc.units.clone(),
                        value: live.clone(),
                        hardware_range: fw_desc.hardware_range,
                        // `firmware_version` lives under
                        // `spec.observables` (it's read-only — there's
                        // no PUT-firmware-version path), so this
                        // reconcile-only insert always materializes
                        // as a non-writable observable in the SPA.
                        writable: false,
                        desired: None,
                        drift: None,
                    });
            }
        }

        let motor = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .actuator_by_role(&motor.common.role)
            .cloned()
            .expect("motor");

        // Operator-owned writable limits: live type-17 read
        {
            let mut params = state.params.write().expect("params poisoned");
            let snap = params
                .entry(motor.common.role.clone())
                .or_insert_with(|| ParamSnapshot {
                    role: motor.common.role.clone(),
                    values: BTreeMap::new(),
                });
            for (name, desc) in &spec.firmware_limits {
                let value = self.read_param_value(&motor, name, desc)?;
                match snap.values.get_mut(name) {
                    Some(pv) => pv.value = value,
                    None => {
                        snap.values.insert(
                            name.clone(),
                            ParamValue {
                                name: name.clone(),
                                index: desc.index,
                                ty: desc.ty.clone(),
                                units: desc.units.clone(),
                                value,
                                hardware_range: desc.hardware_range,
                                // This loop iterates `spec.firmware_limits`
                                // exclusively — the writable side of the
                                // catalog. Hard-code `true` rather than
                                // re-deriving from `desc` so a future spec
                                // change that drops `hardware_range` from a
                                // writable param can't silently flip it
                                // read-only in the SPA.
                                writable: true,
                                desired: None,
                                drift: None,
                            },
                        );
                    }
                }
            }
        }

        let motor = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .actuator_by_role(&motor.common.role)
            .cloned()
            .expect("motor");
        let mut params = state.params.write().expect("params poisoned");
        if let Some(snap) = params.get_mut(&motor.common.role) {
            let n = crate::param_sync::decorate_snapshot(&motor, &spec, snap);
            state
                .drift_counts
                .write()
                .expect("drift_counts poisoned")
                .insert(motor.common.role.clone(), n);
        }
        Ok(())
    }

    pub(crate) fn read_named_f32(
        &self,
        state: &SharedState,
        motor: &Actuator,
        name: &str,
    ) -> Result<Option<f32>> {
        let spec = state.spec_for(motor.robstride_model());
        let desc = spec
            .observables
            .get(name)
            .ok_or_else(|| anyhow!("observable {name} not found in actuator spec"))?;
        let handle = self.handle_for(&motor.common.can_bus)?;
        let bytes =
            handle.read_param(self.host_id, motor.common.can_id, desc.index, PARAM_TIMEOUT)?;
        Ok(bytes.map(f32::from_le_bytes))
    }

    pub(crate) fn read_named_u32(
        &self,
        state: &SharedState,
        motor: &Actuator,
        name: &str,
    ) -> Result<Option<u32>> {
        let spec = state.spec_for(motor.robstride_model());
        let desc = spec
            .observables
            .get(name)
            .ok_or_else(|| anyhow!("observable {name} not found in actuator spec"))?;
        let handle = self.handle_for(&motor.common.can_bus)?;
        let bytes =
            handle.read_param(self.host_id, motor.common.can_id, desc.index, PARAM_TIMEOUT)?;
        Ok(bytes.map(u32::from_le_bytes))
    }

    fn read_param_value(
        &self,
        motor: &Actuator,
        _name: &str,
        desc: &ParamDescriptor,
    ) -> Result<serde_json::Value> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        match desc.ty.as_str() {
            "float" | "f32" | "f64" => Ok(handle
                .read_param(self.host_id, motor.common.can_id, desc.index, PARAM_TIMEOUT)?
                .map(f32::from_le_bytes)
                .map(|v| serde_json::json!(v))
                .unwrap_or(serde_json::Value::Null)),
            "uint8" | "u8" => Ok(handle
                .read_param(self.host_id, motor.common.can_id, desc.index, PARAM_TIMEOUT)?
                .map(|b| b[0])
                .map(|v| serde_json::json!(v))
                .unwrap_or(serde_json::Value::Null)),
            "uint16" | "u16" => Ok(handle
                .read_param(self.host_id, motor.common.can_id, desc.index, PARAM_TIMEOUT)?
                .map(u32::from_le_bytes)
                .map(|v| serde_json::json!(v as u16))
                .unwrap_or(serde_json::Value::Null)),
            "uint32" | "u32" => Ok(handle
                .read_param(self.host_id, motor.common.can_id, desc.index, PARAM_TIMEOUT)?
                .map(u32::from_le_bytes)
                .map(|v| serde_json::json!(v))
                .unwrap_or(serde_json::Value::Null)),
            "string" => Ok(handle
                .read_param(self.host_id, motor.common.can_id, desc.index, PARAM_TIMEOUT)?
                .map(|b| {
                    serde_json::Value::String(
                        String::from_utf8_lossy(&b)
                            .trim_end_matches('\0')
                            .trim()
                            .to_string(),
                    )
                })
                .unwrap_or(serde_json::Value::Null)),
            _ => Ok(serde_json::Value::Null),
        }
    }
}

/// Persist an observed firmware string when it diverges from `inventory.yaml`.
fn update_actuator_firmware_version_yaml(
    state: &crate::state::SharedState,
    inv_path: &Path,
    role: &str,
    version: String,
) -> Result<()> {
    use crate::audit::{AuditEntry, AuditResult};
    use crate::inventory::{self, Device};
    use anyhow::anyhow;
    use chrono::Utc;

    let role_owned = role.to_string();
    let new_inv = inventory::write_atomic(inv_path, |inv| {
        let actuator = inv
            .devices
            .iter_mut()
            .find_map(|device| match device {
                Device::Actuator(a) if a.common.role == role_owned => Some(a),
                _ => None,
            })
            .ok_or_else(|| anyhow!("role {role_owned} not found for firmware_version update"))?;
        actuator.common.firmware_version = Some(version.clone());
        Ok(())
    })?;
    *state.inventory.write().expect("inventory poisoned") = new_inv;

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "firmware_version_observed".into(),
        target: Some(role.to_string()),
        details: serde_json::json!({ "firmware_version": version }),
        result: AuditResult::Ok,
    });
    Ok(())
}
