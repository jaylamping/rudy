//! CAN ownership layer.
//!
//! Phase 1 plan:
//! - On Linux with `cfg.can.mock = false`, open real SocketCAN via the
//!   `driver` crate's `socketcan_bus::CanBus` and pump `driver::rs03::session`
//!   calls to/from it.
//! - Otherwise (any non-Linux host, or explicit mock mode), spawn a mock
//!   generator so the full REST + WebTransport stack is exercisable without
//!   hardware.
//!
use std::sync::Arc;

use anyhow::Result;
use tracing::info;
#[cfg(not(target_os = "linux"))]
use tracing::warn;

use crate::config::Config;
use crate::inventory::Inventory;
#[cfg(not(target_os = "linux"))]
use crate::inventory::Motor;
#[cfg(not(target_os = "linux"))]
use crate::spec::ParamDescriptor;
use crate::state::SharedState;

pub mod backoff;
pub mod mock;
pub mod motion;
pub mod slow_ramp;
pub mod travel;

#[cfg(target_os = "linux")]
pub mod bus_worker;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::LinuxCanCore as RealCanHandle;

#[cfg(not(target_os = "linux"))]
#[derive(Debug)]
pub struct RealCanHandle;

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
impl RealCanHandle {
    pub fn write_param(
        &self,
        _motor: &Motor,
        _desc: &ParamDescriptor,
        _value: &serde_json::Value,
        _save_after: bool,
    ) -> Result<serde_json::Value> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn enable(&self, _motor: &Motor) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn stop(&self, _motor: &Motor) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn save_to_flash(&self, _motor: &Motor) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn set_zero(&self, _motor: &Motor) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    /// Mock-CAN equivalent of [`linux::LinuxCanCore::read_add_offset`].
    ///
    /// Per the commissioned-zero plan, the mock equivalent returns
    /// `Ok(0.0)` so contract tests that exercise the commission
    /// endpoint and boot orchestrator on non-Linux dev hosts (mac /
    /// Windows) don't need a real CAN bus. This is *only* useful for
    /// macOS-style developer tests that hold a `RealCanHandle` directly
    /// — the production daemon path uses `state.real_can = None` for
    /// mock mode, and call sites short-circuit before reaching this
    /// stub. See `crates/rudydae/src/can/mod.rs` `non_linux_stub_tests`
    /// for the test that pins this behavior.
    pub fn read_add_offset(&self, _state: &SharedState, _motor: &Motor) -> Result<f32> {
        Ok(0.0)
    }

    pub fn write_add_offset_persisted(
        &self,
        _state: &SharedState,
        _motor: &Motor,
        _value_rad: f32,
    ) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn set_velocity_setpoint(&self, _motor: &Motor, _vel_rad_s: f32) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn refresh_all_params(&self, _state: &SharedState) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }

    pub fn poll_once(&self, _state: &SharedState) -> Result<()> {
        anyhow::bail!("real CAN is only available on Linux targets")
    }
}

// `inventory` is only consumed by the linux branch below; keep the parameter
// name for non-linux dev builds so the docs read naturally.
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
pub fn build_handle(cfg: &Config, inventory: &Inventory) -> Result<Option<Arc<RealCanHandle>>> {
    #[cfg(target_os = "linux")]
    {
        if !cfg.can.mock {
            return Ok(Some(Arc::new(linux::LinuxCanCore::open(cfg, inventory)?)));
        }
    }

    #[cfg(not(target_os = "linux"))]
    if !cfg.can.mock {
        warn!("rudydae: real CAN requested on a non-Linux target; using mock CAN");
    }

    Ok(None)
}

pub fn spawn(state: SharedState) -> Result<()> {
    if state.cfg.can.mock {
        info!("rudydae: starting mock CAN core");
        return mock::spawn(state);
    }

    // Real CAN: start the dedicated per-bus worker threads. They own
    // their `CanBus` exclusively, stream type-2 frames into
    // `state.latest`, and service `Cmd::*` requests from the API
    // handlers.
    #[cfg(target_os = "linux")]
    if let Some(core) = state.real_can.clone() {
        core.start_workers(&state)?;
    }

    info!("rudydae: real SocketCAN core initialized");
    Ok(())
}
