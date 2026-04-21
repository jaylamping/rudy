//! One-shot v1 → v2 inventory migration. Reads `config/actuators/inventory.yaml`,
//! writes `config/actuators/inventory.yaml.v2`, prints a YAML diff summary to stdout.

use std::env;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config/actuators/inventory.yaml"));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("inventory.yaml");
    let out_path = path.with_file_name(format!("{file_name}.v2"));

    if out_path.exists() {
        bail!(
            "refusing to overwrite existing {}; delete or move it first",
            out_path.display()
        );
    }

    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    let inv = cortex::inventory::migrate_v1_yaml_to_v2_inventory(&text)
        .context("migrate v1 → v2 (check schema_version: 1 and motors: list)")?;

    let out_yaml = serde_yaml::to_string(&inv).context("serialize v2 inventory")?;
    std::fs::write(&out_path, &out_yaml)
        .with_context(|| format!("writing {}", out_path.display()))?;

    println!("Wrote {}", out_path.display());
    println!("--- v2 preview (first 2000 chars) ---");
    let preview: String = out_yaml.chars().take(2000).collect();
    println!("{preview}");
    if out_yaml.len() > 2000 {
        println!("... (truncated)");
    }

    Ok(())
}
