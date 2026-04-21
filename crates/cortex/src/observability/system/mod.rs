//! Host-system metrics poller for the operator console.

mod poller;

pub use poller::{spawn, SystemPoller};
