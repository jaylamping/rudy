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
    /// Everything else in the YAML entry. Preserved for server-side logic
    /// but opaque to the UI (hence ts(skip)).
    #[serde(flatten)]
    #[ts(skip)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
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
