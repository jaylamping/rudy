//! Small shared helpers.

use axum::http::HeaderMap;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

/// Header the SPA mints per browser tab and sends on every mutating request.
/// The daemon's single-operator lock keys off it; missing header ≡ "no
/// session id supplied" which the lock check treats as never-the-holder.
pub const SESSION_HEADER: &str = "X-Rudy-Session";

/// Extract the per-tab session id from a request's headers, if present.
pub fn session_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

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
