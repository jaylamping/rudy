//! Linux SocketCAN core - real hardware path.
//!
//! Each `[[can.buses]]` entry runs on a dedicated I/O thread (see
//! [`crate::can::bus_worker`]). `LinuxCanCore` is the synchronous facade
//! that the rest of the daemon talks to: every public method here builds
//! a [`bus_worker::Cmd`], submits it to the appropriate per-bus channel,
//! and (where a reply is expected) blocks on a oneshot.
//!
//! The previous implementation used a per-iface `Mutex<CanBus>` that
//! every operation had to take in turn. That serialised
//! `set_velocity_setpoint` against `read_param` against the periodic
//! `drain_motor_feedback`, which under sweep cadence (~20 Hz of
//! velocity setpoints + 4 type-17 round-trips per motor per tick) used
//! the whole bus budget on lock contention. The dedicated-thread design
//! also lets the worker stream every received type-2 frame straight
//! into `state.latest`, so the safety check in `api/jog.rs` always sees
//! native-cadence feedback even while a slow `read_param` is in
//! flight.

#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use driver::CanBus;

use crate::boot_state::{self, BootState, ClassifyOutcome};
use crate::can::auto_recovery;
use crate::can::backoff::MotorBackoff;
use crate::can::bus_worker::{self, BusHandle, WriteValue};
use crate::config::Config;
use crate::inventory::{Inventory, Motor};
use crate::spec::ParamDescriptor;
use crate::state::{AppState, SharedState};
use crate::types::{MotorFeedback, ParamSnapshot, ParamValue};

const DEFAULT_HOST_ID: u8 = 0xFD;
const PARAM_TIMEOUT: Duration = Duration::from_millis(30);

/// Result of one type-17 sweep across the auxiliary observables.
/// Each field is `None` when the motor returned a read-fail status
/// (or, for `fault_sta`, when the firmware doesn't expose the
/// shadow). The merge step picks per-field winners between this and
/// the live type-2 row in `state.latest`.
#[derive(Debug, Clone, Copy)]
struct AuxObservables {
    mech_pos: Option<f32>,
    mech_vel: Option<f32>,
    vbus: Option<f32>,
    fault_sta: Option<u32>,
}

/// Two-phase lifecycle:
///
/// 1. [`LinuxCanCore::open`] opens every bus and parks the [`CanBus`]
///    sockets inside `pending_buses`. No worker threads are spawned yet
///    — at this point `AppState` doesn't exist, so we can't hand the
///    workers a `Weak<AppState>` for the type-2 fan-out.
/// 2. [`LinuxCanCore::start_workers`] is called from `can::spawn` once
///    `AppState` is built. It moves each `CanBus` out of `pending_buses`
///    into a freshly-spawned worker thread and stashes the resulting
///    [`BusHandle`] in `handles`.
///
/// Calls into `enable` / `set_velocity_setpoint` / etc. between the two
/// phases would fail with `BusNotReady`; in practice they never happen
/// because every public caller is downstream of `AppState`.
pub struct LinuxCanCore {
    pending_buses: Mutex<BTreeMap<String, CanBus>>,
    handles: OnceLock<BTreeMap<String, BusHandle>>,
    cfg: Config,
    host_id: u8,
    backoff: MotorBackoff,
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
            pending_buses: Mutex::new(buses),
            handles: OnceLock::new(),
            cfg: cfg.clone(),
            host_id: DEFAULT_HOST_ID,
            backoff: MotorBackoff::new(),
        })
    }

    /// Phase 2: spawn one worker per bus. Idempotent — re-calling is a
    /// no-op (the `OnceLock` guards against double-spawn). Auto-assigns
    /// CPU affinity (cores 1..N round-robin, leaving core 0 to the
    /// kernel + tokio) for any bus that omitted `cpu_pin` in its
    /// `[[can.buses]]` entry.
    pub fn start_workers(&self, state: &SharedState) -> Result<()> {
        if self.handles.get().is_some() {
            return Ok(());
        }
        let mut taken = self.pending_buses.lock().expect("pending_buses poisoned");
        // Move all buses out, leaving the map empty so a second call is
        // a clean no-op (it'll find handles already populated above).
        let mut drained: BTreeMap<String, CanBus> = std::mem::take(&mut *taken);
        drop(taken);

        let weak: Weak<AppState> = Arc::downgrade(state);
        let cpu_count = available_cpus();
        let mut handles = BTreeMap::new();
        for (idx, bus_cfg) in self.cfg.can.buses.iter().enumerate() {
            let bus = drained.remove(&bus_cfg.iface).ok_or_else(|| {
                anyhow!(
                    "internal: bus {} disappeared between open and start_workers",
                    bus_cfg.iface
                )
            })?;
            let cpu_pin = bus_cfg
                .cpu_pin
                .or_else(|| bus_worker::auto_assign_cpu(idx, cpu_count));
            let handle = bus_worker::spawn(bus_cfg.iface.clone(), bus, weak.clone(), cpu_pin)
                .with_context(|| format!("spawning worker for {}", bus_cfg.iface))?;
            handles.insert(bus_cfg.iface.clone(), handle);
        }
        self.handles
            .set(handles)
            .map_err(|_| anyhow!("start_workers raced against itself"))?;
        Ok(())
    }

    fn handle_for(&self, iface: &str) -> Result<&BusHandle> {
        self.handles
            .get()
            .ok_or_else(|| anyhow!("bus workers not started yet"))?
            .get(iface)
            .ok_or_else(|| anyhow!("SocketCAN iface {iface} not configured"))
    }

    pub fn write_param(
        &self,
        motor: &Motor,
        desc: &ParamDescriptor,
        value: &serde_json::Value,
        save_after: bool,
    ) -> Result<serde_json::Value> {
        let handle = self.handle_for(&motor.can_bus)?;
        let normalized: serde_json::Value = match desc.ty.as_str() {
            "float" | "f32" | "f64" => {
                let v = value
                    .as_f64()
                    .ok_or_else(|| anyhow!("expected numeric JSON value for {}", desc.ty))?
                    as f32;
                handle.write_param(self.host_id, motor.can_id, desc.index, WriteValue::F32(v))?;
                serde_json::json!(v)
            }
            "uint8" | "u8" => {
                let v = value
                    .as_u64()
                    .ok_or_else(|| anyhow!("expected unsigned integer JSON value"))?;
                let v = u8::try_from(v).context("u8 parameter out of range")?;
                handle.write_param(self.host_id, motor.can_id, desc.index, WriteValue::U8(v))?;
                serde_json::json!(v)
            }
            "uint16" | "u16" => {
                let v = value
                    .as_u64()
                    .ok_or_else(|| anyhow!("expected unsigned integer JSON value"))?;
                let v = u16::try_from(v).context("u16 parameter out of range")?;
                handle.write_param(
                    self.host_id,
                    motor.can_id,
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
                handle.write_param(self.host_id, motor.can_id, desc.index, WriteValue::U32(v))?;
                serde_json::json!(v)
            }
            other => bail!("writes for parameter type {other} are not supported"),
        };

        if save_after {
            handle.save_params(self.host_id, motor.can_id)?;
        }

        Ok(normalized)
    }

    pub fn enable(&self, motor: &Motor) -> Result<()> {
        let handle = self.handle_for(&motor.can_bus)?;
        handle.enable(self.host_id, motor.can_id)?;
        Ok(())
    }

    pub fn stop(&self, motor: &Motor) -> Result<()> {
        let handle = self.handle_for(&motor.can_bus)?;
        handle.stop(self.host_id, motor.can_id)?;
        Ok(())
    }

    pub fn save_to_flash(&self, motor: &Motor) -> Result<()> {
        let handle = self.handle_for(&motor.can_bus)?;
        handle.save_params(self.host_id, motor.can_id)?;
        Ok(())
    }

    pub fn set_zero(&self, motor: &Motor) -> Result<()> {
        let handle = self.handle_for(&motor.can_bus)?;
        handle.set_zero(self.host_id, motor.can_id)?;
        Ok(())
    }

    /// Velocity-mode setpoint. The worker thread implements smart
    /// re-arm: on the first frame after `state.enabled` does NOT
    /// contain the role, the worker writes `RUN_MODE = 2` + sends
    /// `cmd_enable` + writes `SPD_REF`. On every subsequent frame
    /// (`state.enabled` already contains the role), it writes only
    /// `SPD_REF`. Cuts steady-state jog traffic from 60 to 20 frames/s.
    ///
    /// Velocity is *clamped* to the firmware-level `limit_spd`
    /// envelope before forwarding so a misbehaving caller can't bypass
    /// the firmware guard via the REST layer.
    pub fn set_velocity_setpoint(&self, motor: &Motor, vel_rad_s: f32) -> Result<()> {
        let handle = self.handle_for(&motor.can_bus)?;
        handle.set_velocity(self.host_id, motor.can_id, &motor.role, vel_rad_s)?;
        Ok(())
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

    /// Slow-cadence type-17 sweep that backstops the type-2 stream.
    ///
    /// `mech_pos`, `mech_vel`, `vbus`, and `fault_sta` are read here
    /// every tick. The RS03 only emits type-2 motor-feedback frames
    /// while the motor is enabled and active, so a disabled / idle
    /// motor would otherwise have its `state.latest` row stop updating
    /// — leaving the SPA showing stale or zero telemetry, and the
    /// jog stale-feedback guard refusing all motion until the first
    /// jog frame round-trips a type-2 reply. The type-17 fallback
    /// keeps `state.latest` (and the WT broadcast) live regardless.
    ///
    /// Where a type-2 frame arrived more recently than this poll
    /// completed, the type-2 path's pos / vel / torque / temp values
    /// win — the merge below only writes the type-17 readings when
    /// `state.latest[role].t_ms` is older than this poll's start
    /// timestamp. That preserves the high-cadence type-2 stream as
    /// the canonical source while the motor is active.
    ///
    /// Per-motor failures stay isolated via [`MotorBackoff`].
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
            // Snapshot wall-clock before the bus round-trips so the
            // merge below can correctly decide whether a concurrent
            // type-2 frame already updated the row.
            let poll_started_ms = Utc::now().timestamp_millis();
            match self.read_aux_observables(state, motor) {
                Ok(aux) => {
                    self.backoff.record_success(&motor.role);
                    self.merge_aux_into_latest(state, motor, poll_started_ms, aux);
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

    /// Per-tick type-17 sweep. Returns the four values the type-2
    /// stream cannot reliably backstop:
    ///
    /// - `mech_pos` / `mech_vel`: the motor only emits type-2 frames
    ///   while it's enabled and being commanded, so disabled / idle
    ///   motors need a type-17 fallback or the SPA goes dark and the
    ///   jog stale-feedback guard refuses every motion command.
    /// - `vbus`: not on the type-2 payload at all.
    /// - `fault_sta`: per ADR-0002 NOT reachable via type-17 in
    ///   theory (it lives at 0x3022 in the manual's read-only
    ///   namespace), but the read is best-effort here in case a
    ///   future firmware adds the shadow; success is treated as a
    ///   bonus, failure is silent.
    ///
    /// Each value is `None` when the motor returned a read-fail
    /// status. Errors at the bus / channel layer propagate so the
    /// per-motor backoff can rate-limit the chatter.
    fn read_aux_observables(&self, state: &SharedState, motor: &Motor) -> Result<AuxObservables> {
        let mech_pos = self.read_named_f32(state, motor, "mech_pos")?;
        let mech_vel = self.read_named_f32(state, motor, "mech_vel")?;
        let vbus = self.read_named_f32(state, motor, "vbus")?;
        // `fault_sta` lives at 0x3022 (no type-17 shadow per ADR-0002).
        // The read is kept here as a best-effort: on firmwares that
        // added a shadow we get it for free; on older firmwares the
        // motor returns a read-fail status (Ok(None)) and the
        // request frame times out at the worker's REPLY_TIMEOUT,
        // surfacing here as a TimedOut io::Error. Either way we
        // want to keep refreshing the rest of the row, so swallow
        // both into `None`. Channel / bus errors that are NOT a
        // simple per-read timeout still propagate so the per-motor
        // backoff can rate-limit the chatter.
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

    /// Splice freshly-polled aux observables into `state.latest[role]`,
    /// taking care not to clobber a fresher type-2 frame, and broadcast
    /// the resulting row on `feedback_tx` so SPA subscribers see the
    /// update at the poll cadence even when the motor is silent.
    fn merge_aux_into_latest(
        &self,
        state: &SharedState,
        motor: &Motor,
        poll_started_ms: i64,
        aux: AuxObservables,
    ) {
        // Mirror into the params snapshot too so /api/motors/:role/params
        // reflects the most recent observables even between full refresh
        // sweeps.
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
                    },
                );
            }
        }

        // Build the row to insert / merge, then drop the write lock
        // before broadcasting. We do classification + recovery + the
        // broadcast send outside any held lock so a slow subscriber
        // can never wedge the poll loop.
        let merged: MotorFeedback = {
            let mut latest = state.latest.write().expect("latest poisoned");
            let now_ms = Utc::now().timestamp_millis();
            match latest.get_mut(&motor.role) {
                Some(row) => {
                    // vbus / fault_sta are NOT on the type-2 stream;
                    // always overwrite when the read succeeded.
                    if let Some(v) = aux.vbus {
                        row.vbus_v = v;
                    }
                    if let Some(f) = aux.fault_sta {
                        row.fault_sta = f;
                    }
                    // pos / vel ride type-2 too. Only adopt the type-17
                    // value when nothing fresher has landed since this
                    // poll started — otherwise we'd backdate a
                    // 60-Hz-cadence type-2 frame with a slower type-17
                    // reading. `t_ms < poll_started_ms` means "no
                    // type-2 frame arrived during this tick".
                    if row.t_ms < poll_started_ms {
                        if let Some(p) = aux.mech_pos {
                            row.mech_pos_rad = p;
                        }
                        if let Some(v) = aux.mech_vel {
                            row.mech_vel_rad_s = v;
                        }
                        // Stamp the row with the current wall-clock so
                        // the jog stale-feedback guard treats the type-17
                        // reading as fresh telemetry. Without this the
                        // guard would refuse motion on any motor that's
                        // been idle long enough for the last type-2 to
                        // age past `safety.max_feedback_age_ms`.
                        row.t_ms = now_ms;
                    }
                    row.clone()
                }
                None => {
                    // No type-2 has landed yet — seed the row from
                    // whatever the type-17 sweep returned. Anything
                    // that came back `None` (read-fail) starts at 0;
                    // the next successful poll fills it in.
                    let seeded = MotorFeedback {
                        t_ms: now_ms,
                        role: motor.role.clone(),
                        can_id: motor.can_id,
                        mech_pos_rad: aux.mech_pos.unwrap_or_default(),
                        mech_vel_rad_s: aux.mech_vel.unwrap_or_default(),
                        torque_nm: 0.0,
                        vbus_v: aux.vbus.unwrap_or_default(),
                        temp_c: 0.0,
                        fault_sta: aux.fault_sta.unwrap_or_default(),
                        warn_sta: 0,
                    };
                    latest.insert(motor.role.clone(), seeded.clone());
                    seeded
                }
            }
        };

        // Run the boot-state classifier on the freshly-merged position
        // so a motor that drifted out of band while disabled (or
        // simply hasn't been classified yet because no type-2 arrived
        // since boot) transitions into `OutOfBand` and triggers the
        // auto-recovery path. Mirrors what `bus_worker::apply_type2`
        // does on the type-2 hot path.
        if let ClassifyOutcome::Changed {
            new: BootState::OutOfBand { mech_pos_rad, .. },
            ..
        } = boot_state::classify(state, &motor.role, merged.mech_pos_rad)
        {
            auto_recovery::maybe_spawn_recovery(state, &motor.role, mech_pos_rad);
        }

        // Fan out to the WT subscribers so the SPA's live position
        // tracker keeps moving even while the motor is disabled.
        // Errors here mean nobody is listening, which is fine.
        let _ = state.feedback_tx.send(merged);
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
        let handle = self.handle_for(&motor.can_bus)?;
        let bytes = handle.read_param(self.host_id, motor.can_id, desc.index, PARAM_TIMEOUT)?;
        Ok(bytes.map(f32::from_le_bytes))
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
        let handle = self.handle_for(&motor.can_bus)?;
        let bytes = handle.read_param(self.host_id, motor.can_id, desc.index, PARAM_TIMEOUT)?;
        Ok(bytes.map(u32::from_le_bytes))
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

        let handle = self.handle_for(&motor.can_bus)?;
        match desc.ty.as_str() {
            "float" | "f32" | "f64" => Ok(handle
                .read_param(self.host_id, motor.can_id, desc.index, PARAM_TIMEOUT)?
                .map(f32::from_le_bytes)
                .map(|v| serde_json::json!(v))
                .unwrap_or(serde_json::Value::Null)),
            "uint8" | "u8" => Ok(handle
                .read_param(self.host_id, motor.can_id, desc.index, PARAM_TIMEOUT)?
                .map(|b| b[0])
                .map(|v| serde_json::json!(v))
                .unwrap_or(serde_json::Value::Null)),
            "uint16" | "u16" => Ok(handle
                .read_param(self.host_id, motor.can_id, desc.index, PARAM_TIMEOUT)?
                .map(u32::from_le_bytes)
                .map(|v| serde_json::json!(v as u16))
                .unwrap_or(serde_json::Value::Null)),
            "uint32" | "u32" => Ok(handle
                .read_param(self.host_id, motor.can_id, desc.index, PARAM_TIMEOUT)?
                .map(u32::from_le_bytes)
                .map(|v| serde_json::json!(v))
                .unwrap_or(serde_json::Value::Null)),
            _ => Ok(serde_json::Value::Null),
        }
    }

    /// Borrow the raw [`CanBus`] for the duration of `f`, exclusively.
    /// Used by the bench-routine handler in `api/tests.rs`, which needs
    /// to drive the `driver::rs03::tests::run_*` helpers directly
    /// against `&CanBus` for seconds at a time. While `f` runs, the
    /// per-bus worker thread will block on its next `recv` lock attempt
    /// — type-2 streaming pauses for the duration. That matches the
    /// pre-refactor semantic of the per-iface mutex; benches are not
    /// run during operator-driven motion, so the safety surface is
    /// unchanged.
    pub fn with_bus_for_test<T>(
        &self,
        iface: &str,
        f: impl FnOnce(&CanBus) -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        let handle = self
            .handle_for(iface)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, format!("{e:#}")))?;
        handle.with_exclusive_bus(f)
    }
}

fn available_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
