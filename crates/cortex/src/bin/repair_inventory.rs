//! Fix actuator `role` strings so they match `limb` + `joint_kind` (canonical
//! `limb.joint_kind`) when both are set. Use after a manual YAML edit skewed
//! the fields, or to sync a robot checkout before `cortex` starts.
//!
//! Usage:
//!   cargo run -p cortex --bin repair_inventory -- [path/to/inventory.yaml]
//!   cargo run -p cortex --bin repair_inventory -- --dry-run
//!
//! Default path: `config/actuators/inventory.yaml` (run from repo root).

use std::path::PathBuf;

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dry_run = args.iter().any(|a| a == "--dry-run" || a == "-n");
    args.retain(|a| a != "--dry-run" && a != "-n");
    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!(
            "Usage: repair_inventory [--dry-run|-n] [inventory.yaml]\n\n\
             Aligns each actuator's `role` with limb + joint_kind. \
             Default path: config/actuators/inventory.yaml"
        );
        return Ok(());
    }

    let path: PathBuf = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config/actuators/inventory.yaml"));

    let mut inv = cortex::inventory::Inventory::load(&path)
        .with_context(|| format!("loading {}", path.display()))?;
    let changes = cortex::inventory::repair_canonical_actuator_roles(&mut inv)
        .context("repair_canonical_actuator_roles")?;
    if changes.is_empty() {
        println!(
            "No actuator rows needed role repair (or none have both limb and joint_kind set)."
        );
        return Ok(());
    }
    for (old, new) in &changes {
        println!("{old} -> {new}");
    }
    inv.validate()
        .context("post-repair validate() (unexpected — file a bug)")?;

    if dry_run {
        println!("--dry-run: not writing.");
        return Ok(());
    }

    cortex::inventory::write_replace(&path, &inv)
        .with_context(|| format!("writing {}", path.display()))?;
    println!("Wrote {}", path.display());
    Ok(())
}
