//! Small shared helpers.

use serde::{de::Error, Deserialize, Deserializer};

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
