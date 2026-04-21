//! Small shared helpers (session header; see `http/headers`).
//!
//! Flexible YAML `u8` deserialization lives in `hardware::inventory::role`.

pub use crate::http::headers::{session_from_headers, SESSION_HEADER};
