//! Typed loader for `config/actuators/robstride_rs03.yaml`.
//!
//! Only the subset rudyd needs today is modelled; the YAML may contain more
//! fields than we parse (they're ignored).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActuatorSpec {
    #[serde(default)]
    pub schema_version: Option<u32>,
    pub actuator_model: String,
    #[serde(default)]
    pub firmware_limits: BTreeMap<String, ParamDescriptor>,
    #[serde(default)]
    pub observables: BTreeMap<String, ParamDescriptor>,
    #[serde(default)]
    pub commissioning_defaults: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../link/src/api/generated/")]
pub struct ParamDescriptor {
    /// Hex index like `0x700B`, stored as u16 after parsing.
    #[serde(deserialize_with = "de_hex_u16")]
    pub index: u16,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub units: Option<String>,
    /// Present on firmware_limits entries only.
    #[serde(default)]
    pub hardware_range: Option<[f32; 2]>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub values: Option<BTreeMap<String, u32>>,
}

fn de_hex_u16<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    use serde::de::Error;
    let s: serde_yaml::Value = serde::Deserialize::deserialize(d)?;
    match s {
        serde_yaml::Value::Number(n) => n
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .ok_or_else(|| Error::custom("index out of u16 range")),
        serde_yaml::Value::String(s) => {
            let s = s.trim();
            let stripped = s
                .strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .unwrap_or(s);
            u16::from_str_radix(stripped, 16)
                .map_err(|e| Error::custom(format!("parse hex {s}: {e}")))
        }
        _ => Err(Error::custom("expected number or hex string")),
    }
}

impl ActuatorSpec {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let spec: ActuatorSpec = serde_yaml::from_str(&text)
            .with_context(|| format!("parsing YAML in {}", path.display()))?;
        Ok(spec)
    }

    /// Full parameter catalog (firmware_limits + observables) suitable for the UI.
    pub fn catalog(&self) -> Vec<(String, ParamDescriptor)> {
        let mut out = Vec::with_capacity(self.firmware_limits.len() + self.observables.len());
        for (name, d) in &self.firmware_limits {
            out.push((name.clone(), d.clone()));
        }
        for (name, d) in &self.observables {
            out.push((name.clone(), d.clone()));
        }
        out
    }
}
