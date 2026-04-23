//! Linux SocketCAN core — real hardware path.
//!
//! Each `[[can.buses]]` entry runs on a dedicated I/O thread (see
//! [`crate::can::worker`]). `LinuxCanCore` is the synchronous facade
//! that the rest of the daemon talks to: every public method here builds
//! a [`worker::Cmd`], submits it to the appropriate per-bus channel,
//! and (where a reply is expected) blocks on a oneshot.

#![cfg(target_os = "linux")]

mod lifecycle;
mod motion;
mod offset;
mod params;
mod poll;
mod scan;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use driver::CanBus;

use crate::can::worker::{self as bus_worker, BusHandle};
use crate::config::Config;
use crate::inventory::Inventory;
use crate::state::{AppState, SharedState};

pub(crate) use scan::run_hardware_scan;
pub(crate) use scan::run_single_id_probe;

use crate::can::backoff::MotorBackoff;

pub(super) const DEFAULT_HOST_ID: u8 = 0xFD;
/// Upper bound the `BusHandle::read_param` caller waits on the worker
/// reply channel (`recv_blocking(..., timeout + 20ms)` in `handle.rs`).
///
/// Must exceed worst-case **queue stall** before the worker dequeues a
/// `ReadParam`: another in-flight command can hold the worker first — in
/// particular `Cmd::SetVelocity` re-arm sleeps **~60 ms** (two 30 ms
/// firmware settle windows) plus CAN I/O. When this was 30 ms, the aux
/// telemetry poll (`poll_once` → `read_named_f32`) routinely timed out with
/// `timed out waiting on channel` while `home_ramp` was on tick 1,
/// tripping `real-CAN telemetry poll failed` / backoff and briefly
/// starving merged telemetry for that role.
pub(super) const PARAM_TIMEOUT: Duration = Duration::from_millis(150);

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
pub struct LinuxCanCore {
    pending_buses: Mutex<BTreeMap<String, CanBus>>,
    handles: OnceLock<BTreeMap<String, BusHandle>>,
    pub(super) cfg: Config,
    pub(super) host_id: u8,
    pub(super) backoff: MotorBackoff,
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

        for motor in inventory.actuators() {
            if !buses.contains_key(&motor.common.can_bus) {
                bail!(
                    "inventory motor {} uses iface {} but that bus is not configured in [[can.buses]]",
                    motor.common.role,
                    motor.common.can_bus
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
        let mut drained: BTreeMap<String, CanBus> = std::mem::take(&mut *taken);
        drop(taken);

        let weak: Weak<AppState> = Arc::downgrade(state);
        let cpu_count = bus_worker::available_cpus();
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

    pub(super) fn handle_for(&self, iface: &str) -> Result<&BusHandle> {
        self.handles
            .get()
            .ok_or_else(|| anyhow!("bus workers not started yet"))?
            .get(iface)
            .ok_or_else(|| anyhow!("SocketCAN iface {iface} not configured"))
    }

    /// Borrow the raw [`CanBus`] for the duration of `f`, exclusively.
    /// Used by the bench-routine handler in `api/motors/bench.rs`, which needs
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
