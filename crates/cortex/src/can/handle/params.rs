use std::collections::BTreeMap;

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
                Ok(snapshot) => {
                    self.backoff.record_success(&motor.common.role);
                    state
                        .params
                        .write()
                        .expect("params poisoned")
                        .insert(motor.common.role.clone(), snapshot);
                }
                Err(e) => {
                    self.backoff.record_failure(&motor.common.role, &e);
                }
            }
        }
        Ok(())
    }

    fn read_full_snapshot(&self, state: &SharedState, motor: &Actuator) -> Result<ParamSnapshot> {
        let spec = state.spec_for(motor.robstride_model());
        let mut values = BTreeMap::new();
        for (name, desc) in spec.catalog() {
            let value = self.read_param_value(motor, &name, &desc)?;
            values.insert(
                name.clone(),
                ParamValue {
                    name,
                    index: desc.index,
                    ty: desc.ty.clone(),
                    units: desc.units.clone(),
                    value,
                    hardware_range: desc.hardware_range,
                },
            );
        }
        Ok(ParamSnapshot {
            role: motor.common.role.clone(),
            values,
        })
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
        name: &str,
        desc: &ParamDescriptor,
    ) -> Result<serde_json::Value> {
        if name == "firmware_version" {
            return Ok(motor
                .common
                .firmware_version
                .clone()
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null));
        }

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
            _ => Ok(serde_json::Value::Null),
        }
    }
}
