//! SQLite runtime settings (safety/telemetry) merged with `cortex.toml` seed.

pub mod data;
mod init;
mod merge;
pub mod registry;
mod validate;

pub use init::{init, RuntimeConfigInit};
pub use merge::{apply_key_from_json, file_defaults_to_kv, merge_from_kv};
pub use validate::{apply_recovery, validate_snapshot};
