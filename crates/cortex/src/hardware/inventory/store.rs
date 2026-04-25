//! Atomic inventory file writes and seed copy.

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::settings::data;

use super::Inventory;

/// Optional runtime DB: after each successful change, the inventory JSON is
/// written to `inventory_doc` in the same file as `settings_kv`. When
/// `mirror_yaml` is `false`, the YAML on disk is **not** written (rare; tests).
pub fn write_atomic(
    path: &Path,
    db: Option<(Arc<Mutex<Connection>>, bool)>,
    mutate: impl FnOnce(&mut Inventory) -> Result<()>,
) -> Result<Inventory> {
    let mut inv = Inventory::load(path)
        .with_context(|| format!("re-reading {} for write_atomic", path.display()))?;
    mutate(&mut inv)?;

    inv.validate()
        .context("post-mutation inventory validation failed")?;

    if let Some((arc, mirror_yaml)) = db {
        let json =
            serde_json::to_string(&inv).context("serialise inventory json for runtime db")?;
        let c = arc
            .lock()
            .map_err(|e| anyhow::anyhow!("runtime db mutex: {e}"))?;
        data::set_inventory_json(&*c, &json).context("persist inventory to runtime SQLite")?;
        drop(c);
        if !mirror_yaml {
            return Ok(inv);
        }
    }

    let yaml = serde_yaml::to_string(&inv).context("serialising inventory back to YAML")?;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("inventory.yaml");

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

    tmp.persist(path)
        .with_context(|| format!("rename tempfile -> {}", path.display()))?;
    Ok(inv)
}

/// Write an already-mutated, validated [`Inventory`] to `path` (replaces the
/// file). Offline tools (e.g. `repair_inventory`) use this to avoid
/// `write_atomic`'s re-load.
pub fn write_replace(path: &Path, inv: &Inventory) -> Result<()> {
    inv.validate()
        .context("write_replace: inventory did not pass validate()")?;
    let yaml = serde_yaml::to_string(inv).context("serialising inventory to YAML")?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("inventory.yaml");
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
    tmp.persist(path)
        .with_context(|| format!("rename tempfile -> {}", path.display()))?;
    Ok(())
}

/// Seed copy helper (unchanged contract from v1).
pub fn ensure_seeded(inventory: &Path, seed: Option<&Path>) -> Result<()> {
    if inventory.exists() {
        return Ok(());
    }
    let Some(seed) = seed else {
        return Ok(());
    };
    if !seed.exists() {
        return Ok(());
    }
    if let Some(parent) = inventory.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent dir {}", parent.display()))?;
        }
    }
    std::fs::copy(seed, inventory).with_context(|| {
        format!(
            "seeding inventory: copy {} -> {}",
            seed.display(),
            inventory.display()
        )
    })?;
    Ok(())
}
