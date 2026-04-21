//! Role string validation (same rules as v1) and flexible u8 deserialization for YAML.

use anyhow::{anyhow, Result};
use serde::de::Error;
use serde::{Deserialize, Deserializer};

/// Accept either a bare YAML integer or a string like "0x08" / "0X7F" / "127".
pub fn de_u8_flex<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    let v: serde_yaml::Value = Deserialize::deserialize(d)?;
    match v {
        serde_yaml::Value::Number(n) => n
            .as_u64()
            .and_then(|v| u8::try_from(v).ok())
            .ok_or_else(|| D::Error::custom("u8 out of range")),
        serde_yaml::Value::String(s) => {
            let s = s.trim();
            if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u8::from_str_radix(h, 16).map_err(|e| D::Error::custom(format!("hex u8 {s}: {e}")))
            } else {
                s.parse::<u8>()
                    .map_err(|e| D::Error::custom(format!("dec u8 {s}: {e}")))
            }
        }
        other => Err(D::Error::custom(format!("expected u8, got {other:?}"))),
    }
}

/// `serde::Deserialize` adapter for `de_u8_flex` (ts-rs understands `with`, not `deserialize_with`).
pub mod serde_u8_flex {
    use serde::{Deserializer, Serialize, Serializer};

    pub fn deserialize<'de, D>(d: D) -> Result<u8, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::de_u8_flex(d)
    }

    pub fn serialize<S>(v: &u8, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        v.serialize(s)
    }
}

pub fn validate_role_format(role: &str) -> Result<()> {
    if role.is_empty() {
        return Err(anyhow!("role is empty"));
    }
    let bytes = role.as_bytes();
    if !bytes[0].is_ascii_lowercase() {
        return Err(anyhow!("role {role} must start with a lowercase letter"));
    }
    let dots = role.matches('.').count();
    if dots > 1 {
        return Err(anyhow!("role {role} contains more than one dot"));
    }
    for &b in bytes {
        let ok = b.is_ascii_lowercase() || b == b'_' || b == b'.' || b.is_ascii_digit();
        if !ok {
            return Err(anyhow!(
                "role {role} contains illegal character `{}`",
                b as char
            ));
        }
    }
    Ok(())
}

pub fn validate_canonical_role(role: &str) -> Result<()> {
    validate_role_format(role)?;
    if !role.contains('.') {
        return Err(anyhow!(
            "role {role} is not canonical (expected `{{limb}}.{{joint_kind}}`)"
        ));
    }
    let parts: Vec<&str> = role.split('.').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(anyhow!("role {role} must have exactly one non-empty dot"));
    }
    Ok(())
}
