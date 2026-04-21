//! Inventory load/validation errors.

use thiserror::Error;

/// Structured failure modes for `Inventory::load`.
#[derive(Debug, Error)]
pub enum InventoryError {
    /// On-disk file is not schema v2 — run the migration tool once.
    #[error(
        "inventory schema version mismatch: found {found}, required {required} — {migration_hint}"
    )]
    SchemaVersionMismatch {
        found: u32,
        required: u32,
        migration_hint: String,
    },
}
