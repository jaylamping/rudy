//! Linux SocketCAN core - real hardware path.

#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use driver::rs03::feedback::MotorFeedback as DriverFeedback;
use driver::rs03::session;
use driver::CanBus;

use crate::config::Config;
use crate::inventory::{Inventory, Motor};
use crate::spec::ParamDescriptor;
use crate::state::SharedState;
use crate::types::{MotorFeedback, ParamSnapshot, ParamValue};

const DEFAULT_HOST_ID: u8 = 0xFD;
const READ_TIMEOUT: Duration = Duration::from_millis(5);
const PARAM_TIMEOUT: Duration = Duration::from_millis(30);
const FEEDBACK_DRAIN_TIMEOUT: Duration = Duration::from_millis(2);

pub struct LinuxCanCore {
    inner: Mutex<LinuxCanInner>,
    host_id: u8,
}

struct LinuxCanInner {
    buses: BTreeMap<String, CanBus>,
}

impl LinuxCanCore {
    pub fn open(cfg: &Config, inventory: &Inventory) -> Result<Self> {
        if cfg.can.buses.is_empty() {
            bail!("can.mock=false but no [[can.buses]] entries are configured");
        }

        let mut buses = BTreeMap::new();
        for bus_cfg in &cfg.can.buses {
            let bus = CanBus::open(&bus_cfg.iface)
                .with_context(|| format!("opening SocketCAN iface {}", bus_cfg.iface))?;
            bus.set_read_timeout(READ_TIMEOUT)
                .with_context(|| format!("setting read timeout on {}", bus_cfg.iface))?;
            buses.insert(bus_cfg.iface.clone(), bus);
        }

        for motor in &inventory.motors {
            if !buses.contains_key(&motor.can_bus) {
                bail!(
                    "inventory motor {} uses iface {} but that bus is not configured in [[can.buses]]",
                    motor.role,
                    motor.can_bus
                );
            }
        }

        Ok(Self {
            inner: Mutex::new(LinuxCanInner { buses }),
            host_id: DEFAULT_HOST_ID,
        })
    }

    pub fn write_param(
        &self,
        motor: &Motor,
        desc: &ParamDescriptor,
        value: &serde_json::Value,
        save_after: bool,
    ) -> Result<serde_json::Value> {
        self.with_bus(&motor.can_bus, |bus| {
            let normalized: serde_json::Value = match desc.ty.as_str() {
                "float" | "f32" | "f64" => {
                    let v = value
                        .as_f64()
                        .ok_or_else(|| anyhow!("expected numeric JSON value for {}", desc.ty))?
                        as f32;
                    session::write_param_f32(bus, self.host_id, motor.can_id, desc.index, v)?;
                    serde_json::json!(v)
                }
                "uint8" | "u8" => {
                    let v = value
                        .as_u64()
                        .ok_or_else(|| anyhow!("expected unsigned integer JSON value"))?;
                    let v = u8::try_from(v).context("u8 parameter out of range")?;
                    session::write_param_u8(bus, self.host_id, motor.can_id, desc.index, v)?;
                    serde_json::json!(v)
                }
                "uint16" | "u16" => {
                    let v = value
                        .as_u64()
                        .ok_or_else(|| anyhow!("expected unsigned integer JSON value"))?;
                    let v = u16::try_from(v).context("u16 parameter out of range")?;
                    session::write_param_u32(
                        bus,
                        self.host_id,
                        motor.can_id,
                        desc.index,
                        v as u32,
                    )?;
                    serde_json::json!(v)
                }
                "uint32" | "u32" => {
                    let v = value
                        .as_u64()
                        .ok_or_else(|| anyhow!("expected unsigned integer JSON value"))?;
                    let v = u32::try_from(v).context("u32 parameter out of range")?;
                    session::write_param_u32(bus, self.host_id, motor.can_id, desc.index, v)?;
                    serde_json::json!(v)
                }
                other => bail!("writes for parameter type {other} are not supported"),
            };

            if save_after {
                session::cmd_save_params(bus, self.host_id, motor.can_id)?;
            }

            Ok(normalized)
        })
    }

    pub fn enable(&self, motor: &Motor) -> Result<()> {
        self.with_bus(&motor.can_bus, |bus| {
            session::cmd_enable(bus, self.host_id, motor.can_id)?;
            Ok(())
        })
    }

    pub fn stop(&self, motor: &Motor) -> Result<()> {
        self.with_bus(&motor.can_bus, |bus| {
            session::cmd_stop(bus, self.host_id, motor.can_id, false)?;
            Ok(())
        })
    }

    pub fn save_to_flash(&self, motor: &Motor) -> Result<()> {
        self.with_bus(&motor.can_bus, |bus| {
            session::cmd_save_params(bus, self.host_id, motor.can_id)?;
            Ok(())
        })
    }

    pub fn set_zero(&self, motor: &Motor) -> Result<()> {
        self.with_bus(&motor.can_bus, |bus| {
            session::cmd_set_zero(bus, self.host_id, motor.can_id)?;
            Ok(())
        })
    }

    pub fn refresh_all_params(&self, state: &SharedState) -> Result<()> {
        for motor in state.inventory.motors.iter().filter(|m| m.present) {
            let snapshot = self.read_full_snapshot(state, motor)?;
            state
                .params
                .write()
                .expect("params poisoned")
                .insert(motor.role.clone(), snapshot);
        }
        Ok(())
    }

    pub fn poll_once(&self, state: &SharedState) -> Result<()> {
        for motor in state.inventory.motors.iter().filter(|m| m.present) {
            let latest = self.read_live_feedback(state, motor)?;
            state
                .latest
                .write()
                .expect("latest poisoned")
                .insert(motor.role.clone(), latest.clone());
            let _ = state.feedback_tx.send(latest);
        }
        Ok(())
    }

    fn read_full_snapshot(&self, state: &SharedState, motor: &Motor) -> Result<ParamSnapshot> {
        let mut values = BTreeMap::new();
        for (name, desc) in state.spec.catalog() {
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
            role: motor.role.clone(),
            values,
        })
    }

    fn read_live_feedback(&self, state: &SharedState, motor: &Motor) -> Result<MotorFeedback> {
        let mech_pos = self
            .read_named_f32(state, motor, "mech_pos")?
            .unwrap_or_default();
        let mech_vel = self
            .read_named_f32(state, motor, "mech_vel")?
            .unwrap_or_default();
        let vbus = self
            .read_named_f32(state, motor, "vbus")?
            .unwrap_or_default();
        let fault_sta = self
            .read_named_u32(state, motor, "fault_sta")?
            .unwrap_or_default();
        let feedback = self.read_type2_feedback(motor)?;

        {
            let mut params = state.params.write().expect("params poisoned");
            let snapshot = params
                .entry(motor.role.clone())
                .or_insert_with(|| ParamSnapshot {
                    role: motor.role.clone(),
                    values: BTreeMap::new(),
                });
            for (name, desc) in state.spec.observables.iter() {
                let value = match name.as_str() {
                    "mech_pos" => serde_json::json!(mech_pos),
                    "mech_vel" => serde_json::json!(mech_vel),
                    "vbus" => serde_json::json!(vbus),
                    "fault_sta" => serde_json::json!(fault_sta),
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
                    },
                );
            }
        }

        let (torque_nm, temp_c, fb_pos, fb_vel) = feedback
            .map(|fb| {
                (
                    fb.torque_nm,
                    fb.temp_c,
                    Some(fb.pos_rad),
                    Some(fb.vel_rad_s),
                )
            })
            .unwrap_or((0.0, 0.0, None, None));

        Ok(MotorFeedback {
            t_ms: Utc::now().timestamp_millis(),
            role: motor.role.clone(),
            can_id: motor.can_id,
            mech_pos_rad: fb_pos.unwrap_or(mech_pos),
            mech_vel_rad_s: fb_vel.unwrap_or(mech_vel),
            torque_nm,
            vbus_v: vbus,
            temp_c,
            fault_sta,
            warn_sta: 0,
        })
    }

    fn read_named_f32(
        &self,
        state: &SharedState,
        motor: &Motor,
        name: &str,
    ) -> Result<Option<f32>> {
        let desc = state
            .spec
            .observables
            .get(name)
            .ok_or_else(|| anyhow!("observable {name} not found in actuator spec"))?;
        self.with_bus(&motor.can_bus, |bus| {
            session::read_param_f32(bus, self.host_id, motor.can_id, desc.index, PARAM_TIMEOUT)
                .map_err(Into::into)
        })
    }

    fn read_named_u32(
        &self,
        state: &SharedState,
        motor: &Motor,
        name: &str,
    ) -> Result<Option<u32>> {
        let desc = state
            .spec
            .observables
            .get(name)
            .ok_or_else(|| anyhow!("observable {name} not found in actuator spec"))?;
        self.with_bus(&motor.can_bus, |bus| {
            session::read_param_u32(bus, self.host_id, motor.can_id, desc.index, PARAM_TIMEOUT)
                .map_err(Into::into)
        })
    }

    fn read_type2_feedback(&self, motor: &Motor) -> Result<Option<DriverFeedback>> {
        self.with_bus(&motor.can_bus, |bus| {
            session::drain_motor_feedback(bus, self.host_id, motor.can_id, FEEDBACK_DRAIN_TIMEOUT)
                .map_err(Into::into)
        })
    }

    fn read_param_value(
        &self,
        motor: &Motor,
        name: &str,
        desc: &ParamDescriptor,
    ) -> Result<serde_json::Value> {
        if name == "firmware_version" {
            return Ok(motor
                .firmware_version
                .clone()
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null));
        }

        self.with_bus(&motor.can_bus, |bus| match desc.ty.as_str() {
            "float" | "f32" | "f64" => Ok(session::read_param_f32(
                bus,
                self.host_id,
                motor.can_id,
                desc.index,
                PARAM_TIMEOUT,
            )?
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null)),
            "uint8" | "u8" => Ok(session::read_param_u8(
                bus,
                self.host_id,
                motor.can_id,
                desc.index,
                PARAM_TIMEOUT,
            )?
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null)),
            "uint16" | "u16" => Ok(session::read_param_u32(
                bus,
                self.host_id,
                motor.can_id,
                desc.index,
                PARAM_TIMEOUT,
            )?
            .map(|v| serde_json::json!(v as u16))
            .unwrap_or(serde_json::Value::Null)),
            "uint32" | "u32" => Ok(session::read_param_u32(
                bus,
                self.host_id,
                motor.can_id,
                desc.index,
                PARAM_TIMEOUT,
            )?
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null)),
            _ => Ok(serde_json::Value::Null),
        })
    }

    fn with_bus<T>(&self, iface: &str, f: impl FnOnce(&CanBus) -> Result<T>) -> Result<T> {
        let inner = self.inner.lock().expect("linux can mutex poisoned");
        let bus = inner
            .buses
            .get(iface)
            .ok_or_else(|| anyhow!("SocketCAN iface {iface} not configured"))?;
        f(bus)
    }
}
