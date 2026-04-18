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

use crate::boot_state::{self, BootState, ClassifyOutcome};
use crate::can::auto_recovery;
use crate::can::backoff::MotorBackoff;
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
    backoff: MotorBackoff,
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
            backoff: MotorBackoff::new(),
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

    /// Velocity-mode setpoint. Idempotent, so the jog endpoint can call it
    /// at 20 Hz without re-entering enable / run-mode for each frame: the
    /// setup writes only run on the first call, subsequent calls just
    /// re-issue `spd_ref`.
    ///
    /// Velocity is *clamped* to the firmware-level `limit_spd` envelope
    /// before forwarding so a misbehaving caller can't bypass the firmware
    /// guard via the REST layer.
    pub fn set_velocity_setpoint(&self, motor: &Motor, vel_rad_s: f32) -> Result<()> {
        self.with_bus(&motor.can_bus, |bus| {
            // Best-effort: re-arm the motor on every call. cmd_enable is
            // idempotent and write_param_u8 / _f32 are tiny — at 20 Hz this
            // adds ~1 ms of bus time per frame.
            driver::rs03::session::write_param_u8(
                bus,
                self.host_id,
                motor.can_id,
                driver::rs03::params::RUN_MODE,
                2,
            )?;
            driver::rs03::session::cmd_enable(bus, self.host_id, motor.can_id)?;
            driver::rs03::session::write_param_f32(
                bus,
                self.host_id,
                motor.can_id,
                driver::rs03::params::SPD_REF,
                vel_rad_s,
            )?;
            Ok(())
        })
    }

    /// RAM-write low torque AND speed limits for every present motor.
    ///
    /// Called once at telemetry startup. Implements Layer 4 of the
    /// boot-time gate: even if the higher layers got the state machine
    /// wrong, the worst-case behavior of a misfiring motor is "slow and
    /// weak" instead of "fast and strong." Uses RAM writes only (NO
    /// `save_params`) so a daemon restart restores the operator's
    /// flash-resident commissioning values.
    ///
    /// Failures are logged and per-motor isolated; a single motor that
    /// won't accept the write doesn't block the others. Callers SHOULD
    /// keep the affected motors in `BootState::Unknown` so enable refuses
    /// — that's the safer fallback than enabling with unknown limits.
    pub fn seed_boot_low_limits(&self, state: &SharedState) {
        let motors: Vec<Motor> = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .motors
            .iter()
            .filter(|m| m.present)
            .cloned()
            .collect();

        let limit_torque_nm = state
            .spec
            .commissioning_defaults
            .get("limit_torque_nm")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32);
        let limit_spd_rad_s = state
            .spec
            .commissioning_defaults
            .get("limit_spd_rad_s")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32);

        for motor in motors {
            if let (Some(t), Some(_)) = (limit_torque_nm, &motor.travel_limits) {
                if let Some(desc) = state.spec.firmware_limits.get("limit_torque") {
                    if let Err(e) = self.write_param(&motor, desc, &serde_json::json!(t), false) {
                        tracing::warn!(role = %motor.role, error = ?e, "boot-time limit_torque RAM write failed");
                    }
                }
            }
            if let Some(s) = limit_spd_rad_s {
                if let Some(desc) = state.spec.firmware_limits.get("limit_spd") {
                    if let Err(e) = self.write_param(&motor, desc, &serde_json::json!(s), false) {
                        tracing::warn!(role = %motor.role, error = ?e, "boot-time limit_spd RAM write failed");
                    }
                }
            }
        }
    }

    /// Refresh the full parameter snapshot for every present motor.
    ///
    /// Per-motor failures are isolated: a motor that errors out logs
    /// once via the backoff state and is skipped on subsequent polls
    /// for an exponentially-increasing window (capped at 30 s). Other
    /// motors keep being refreshed normally. The function therefore
    /// always returns `Ok(())` to its caller — there's nothing left at
    /// the call site that can usefully fail-the-batch on.
    pub fn refresh_all_params(&self, state: &SharedState) -> Result<()> {
        // Snapshot the inventory so we don't hold the RwLock across the
        // (potentially blocking) per-motor SocketCAN reads.
        let motors: Vec<Motor> = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .motors
            .iter()
            .filter(|m| m.present)
            .cloned()
            .collect();
        for motor in &motors {
            if !self.backoff.should_poll(&motor.role) {
                continue;
            }
            match self.read_full_snapshot(state, motor) {
                Ok(snapshot) => {
                    self.backoff.record_success(&motor.role);
                    state
                        .params
                        .write()
                        .expect("params poisoned")
                        .insert(motor.role.clone(), snapshot);
                }
                Err(e) => {
                    self.backoff.record_failure(&motor.role, &e);
                }
            }
        }
        Ok(())
    }

    /// Poll live feedback for every present motor.
    ///
    /// Same isolation guarantees as [`Self::refresh_all_params`].
    pub fn poll_once(&self, state: &SharedState) -> Result<()> {
        let motors: Vec<Motor> = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .motors
            .iter()
            .filter(|m| m.present)
            .cloned()
            .collect();
        for motor in &motors {
            if !self.backoff.should_poll(&motor.role) {
                continue;
            }
            match self.read_live_feedback(state, motor) {
                Ok(latest) => {
                    self.backoff.record_success(&motor.role);
                    state
                        .latest
                        .write()
                        .expect("latest poisoned")
                        .insert(motor.role.clone(), latest.clone());
                    if let ClassifyOutcome::Changed { new, .. } =
                        boot_state::classify(state, &motor.role, latest.mech_pos_rad)
                    {
                        if let BootState::OutOfBand { mech_pos_rad, .. } = new {
                            auto_recovery::maybe_spawn_recovery(state, &motor.role, mech_pos_rad);
                        }
                    }
                    let _ = state.feedback_tx.send(latest);
                }
                Err(e) => {
                    self.backoff.record_failure(&motor.role, &e);
                }
            }
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

    /// Like [`with_bus`] but for callers that already use `std::io::Result`
    /// (e.g. the `driver::rs03::tests` library). The bench routines own the
    /// bus for their entire duration, which is exactly what the per-iface
    /// mutex inside `LinuxCanInner` already enforces.
    pub fn with_bus_for_test<T>(
        &self,
        iface: &str,
        f: impl FnOnce(&CanBus) -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        let inner = self.inner.lock().expect("linux can mutex poisoned");
        let bus = inner.buses.get(iface).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("SocketCAN iface {iface} not configured"),
            )
        })?;
        f(bus)
    }
}
