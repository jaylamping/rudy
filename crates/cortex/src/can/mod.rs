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
use std::time::Duration;

use anyhow::Result;
use tracing::info;
#[cfg(not(target_os = "linux"))]
use tracing::warn;

use crate::config::Config;
use crate::inventory::Inventory;
use crate::state::SharedState;

pub mod backoff;
pub mod discovery;
pub mod math;
pub mod mock;
/// Legacy path for angle math used across CAN and motion (`can::motion::wrap_to_pi`, …).
pub use math as motion;
pub mod home_ramp;
pub mod travel;

use discovery::HardwareScanReport;

#[cfg(target_os = "linux")]
pub mod worker;
#[cfg(target_os = "linux")]
pub use worker as bus_worker;

#[cfg(target_os = "linux")]
pub mod handle;
#[cfg(target_os = "linux")]
pub use handle as linux;

#[cfg(target_os = "linux")]
pub use handle::LinuxCanCore as RealCanHandle;

#[cfg(not(target_os = "linux"))]
mod stub;

#[cfg(not(target_os = "linux"))]
pub use stub::RealCanHandle;

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
        warn!("cortex: real CAN requested on a non-Linux target; using mock CAN");
    }

    Ok(None)
}

pub fn spawn(state: SharedState) -> Result<()> {
    if state.cfg.can.mock {
        info!("cortex: starting mock CAN core");
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

    info!("cortex: real SocketCAN core initialized");
    Ok(())
}

/// Run active discovery probes on configured buses (no-op on mock / non-Linux).
pub fn hardware_active_scan(
    state: &SharedState,
    bus_filter: Option<&str>,
    id_min: u8,
    id_max: u8,
    timeout: Duration,
) -> anyhow::Result<HardwareScanReport> {
    #[cfg(target_os = "linux")]
    {
        if !state.cfg.can.mock {
            if let Some(core) = state.real_can.as_deref() {
                return linux::run_hardware_scan(core, state, bus_filter, id_min, id_max, timeout);
            }
        }
    }
    let _ = (state, bus_filter, id_min, id_max, timeout);
    Ok(HardwareScanReport::empty(
        "mock or non-Linux build — active scan did not touch the bus",
    ))
}
