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

    info!("rudydae: real SocketCAN core initialized");
    Ok(())
}
