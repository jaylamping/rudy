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
//! Phase 1 ships only the mock path wired end-to-end. The Linux branch is
//! stubbed here with a `todo!()`-style warning + fallback to mock, so the
//! binary compiles and runs on both developer laptops and the Pi today.
//! Replacing the fallback with the real driver wiring is tracked by the
//! `rudydae_can_core` plan task.

use anyhow::Result;
use tracing::info;
#[cfg(target_os = "linux")]
use tracing::warn;

use crate::state::SharedState;

pub mod mock;

#[cfg(target_os = "linux")]
pub mod linux;

pub fn spawn(state: SharedState) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        if !state.cfg.can.mock {
            warn!(
                "rudydae: real SocketCAN path is not yet wired end-to-end; falling back to mock. \
                 See the rudydae_can_core task in docs/decisions/0004-operator-console.md."
            );
            // Once linux::spawn is ready, swap the body of this branch to:
            //   return linux::spawn(state);
            let _ = linux::placeholder;
            return mock::spawn(state);
        }
    }

    info!("rudydae: starting mock CAN core");
    mock::spawn(state)
}
