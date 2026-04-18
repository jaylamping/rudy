//! Typed loader for `config/actuators/inventory.yaml`.
//!
//! We only model the fields rudydae enforces or surfaces in the UI; the rest
//! are tolerated via `#[serde(flatten)]` into a catch-all map so the YAML
//! can grow without breaking rudydae.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    pub schema_version: Option<u32>,
    pub motors: Vec<Motor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct Motor {
    pub role: String,
    pub can_bus: String,
    #[serde(with = "crate::util::serde_u8_flex")]
    #[ts(as = "u8")]
    pub can_id: u8,
    #[serde(default)]
    pub firmware_version: Option<String>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub commissioned_at: Option<String>,
    /// Whether the physical motor is wired into the bus right now.
    ///
    /// Defaults to `true` so existing inventory entries keep behaving as before.
    /// Set to `false` for placeholder entries (motor planned but not yet on the
    /// bus) or for temporarily-removed motors. Affects:
    ///
    ///   * Real-CAN telemetry: rudydae skips polling absent motors so an
    ///     unanswered iface doesn't fill the SocketCAN txqueue and start
    ///     returning ENOBUFS (errno 105) on every send.
    ///   * Control plane: enable / stop / save / set_zero on an absent motor
    ///     are rejected at the API layer with a clean `409 Conflict` rather
    ///     than queuing CAN frames that will never get an ACK.
    ///   * Mock CAN + the REST `/api/motors` listing still include absent
    ///     motors so the UI can show them with an "absent" badge.
    #[serde(default = "default_true")]
    pub present: bool,
    /// Per-motor soft travel-limits band, in radians. None ≡ "use the spec
    /// default" (which is currently the full RS03 ±2-turn envelope from
    /// `protocol.position_min_rad / position_max_rad`).
    ///
    /// Edited via `PUT /api/motors/:role/travel_limits`; persisted by
    /// rewriting `inventory.yaml` atomically (see `inventory::write_atomic`).
    /// Enforced by `crate::can::travel::enforce_travel_band` on every
    /// commanded move (jog now, future move-to).
    #[serde(default)]
    pub travel_limits: Option<TravelLimits>,
    /// Everything else in the YAML entry. Preserved for server-side logic
    /// but opaque to the UI (hence ts(skip)).
    #[serde(flatten)]
    #[ts(skip)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

/// Per-actuator soft travel-limits band (radians).
///
/// Stored on each [`Motor`] in `config/actuators/inventory.yaml` and enforced
/// by rudydae on every commanded move (jog, future move-to). Semantically
/// this is a software-side inner cap; the firmware-level position envelope
/// remains authoritative.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct TravelLimits {
    pub min_rad: f32,
    pub max_rad: f32,
    /// ISO 8601 timestamp (UTC) of the most recent change. None on entries
    /// authored by hand or imported from a pre-rudydae inventory.
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Inventory {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let inv: Inventory = serde_yaml::from_str(&text)
            .with_context(|| format!("parsing YAML in {}", path.display()))?;
        Ok(inv)
    }

    pub fn by_role(&self, role: &str) -> Option<&Motor> {
        self.motors.iter().find(|m| m.role == role)
    }

    #[allow(dead_code)]
    pub fn by_can_id(&self, can_id: u8) -> Option<&Motor> {
        self.motors.iter().find(|m| m.can_id == can_id)
    }
}

/// Atomic YAML rewrite: read the on-disk inventory, hand the parsed value to
/// `mutate`, then write the result to a sibling tempfile and rename it into
/// place. Either the rename succeeds and the new file is fully visible, or
/// it fails and the original file is untouched.
///
/// Used by the per-motor PUT endpoints (`travel_limits`, `verified`) so a
/// crash mid-write can never produce a half-written `inventory.yaml`.
///
/// Returns the post-mutation `Inventory` so callers can refresh in-memory
/// state without a re-read.
pub fn write_atomic(
    path: &Path,
    mutate: impl FnOnce(&mut Inventory) -> Result<()>,
) -> Result<Inventory> {
    let mut inv = Inventory::load(path)
        .with_context(|| format!("re-reading {} for write_atomic", path.display()))?;
    mutate(&mut inv)?;

    let yaml = serde_yaml::to_string(&inv).context("serialising inventory back to YAML")?;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("inventory.yaml");

    // tempfile in the *same* directory so the rename is atomic on POSIX.
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{file_stem}."))
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| format!("creating tempfile next to {}", path.display()))?;

    {
        use std::io::Write;
        tmp.write_all(yaml.as_bytes())
            .context("writing inventory YAML to tempfile")?;
        tmp.as_file()
            .sync_all()
            .context("fsync inventory tempfile")?;
    }

    // `persist` does the rename. On Windows it can fail if the target is
    // open; rudydae never holds a handle to inventory.yaml between writes.
    tmp.persist(path)
        .with_context(|| format!("rename tempfile -> {}", path.display()))?;
    Ok(inv)
}
