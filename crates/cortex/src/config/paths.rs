//! Filesystem paths for specs, inventory, and audit log.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    pub actuator_spec: PathBuf,
    /// Path cortex reads AND writes the live inventory from. Must live on a
    /// writable path that the daemon's user can mutate (e.g. `/var/lib/rudy/`
    /// on the Pi, where systemd `ProtectSystem=strict` permits writes via
    /// `ReadWritePaths`). Editing is via PUT endpoints (`travel_limits`,
    /// `verified`, `rename`); never hand-edit while the daemon is running.
    pub inventory: PathBuf,
    /// Optional read-only seed path. When set and `inventory` does not exist
    /// on disk at startup, cortex copies `inventory_seed` → `inventory`
    /// once. Used on the Pi where `/opt/rudy/config/actuators/inventory.yaml`
    /// ships with the release tarball as the baseline, and `/var/lib/rudy/
    /// inventory.yaml` is the operator-mutable copy that survives upgrades.
    /// Leave unset for dev workflows where `inventory` is in-tree and edited
    /// in your editor.
    #[serde(default)]
    pub inventory_seed: Option<PathBuf>,
    pub audit_log: PathBuf,
}
