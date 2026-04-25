//! Bootstrap: open runtime DB, seed, merge file + DB into effective snapshot.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use rusqlite::Connection;
use tracing::warn;

use super::data;
use super::merge;
use super::validate;
use crate::config::Config;
use crate::config::SafetyConfig;
use crate::config::TelemetryConfig;

const META_RECOVERY: &str = "recovery_pending";
const META_SEEDED: &str = "bootstrapped";

/// Single effective runtime view + DB handle (only when runtime DB enabled).
pub struct RuntimeConfigInit {
    pub effective_safety: SafetyConfig,
    pub effective_telemetry: TelemetryConfig,
    pub recovery_pending: bool,
    /// Only `Some` when `cfg.runtime.enabled` and DB opened.
    pub db: Option<Arc<Mutex<Connection>>>,
}

fn seed_all(conn: &mut Connection, cfg: &Config) -> Result<()> {
    let rows = merge::file_defaults_to_kv(cfg);
    data::replace_all_kv(conn, &rows).context("replace_all_kv")
}

pub fn init(cfg: &Config) -> Result<RuntimeConfigInit> {
    if !cfg.runtime.enabled {
        return Ok(RuntimeConfigInit {
            effective_safety: cfg.safety.clone(),
            effective_telemetry: cfg.telemetry.clone(),
            recovery_pending: false,
            db: None,
        });
    }

    let path = &cfg.runtime.db_path;
    let (mut conn, did_recovery) = open_or_repair(path, cfg)?;

    let has_rows: i64 = conn
        .query_row("SELECT count(*) FROM settings_kv", [], |r| r.get(0))
        .map_err(|e| anyhow::anyhow!(e))?;

    if has_rows == 0 && !did_recovery {
        seed_all(&mut conn, cfg).map_err(|e| anyhow::anyhow!(e))?;
        data::set_meta(&conn, META_SEEDED, "1").map_err(|e| anyhow::anyhow!(e))?;
        data::set_meta(&conn, META_RECOVERY, "0").map_err(|e| anyhow::anyhow!(e))?;
    }

    let recovery_pending = data::get_meta(&conn, META_RECOVERY)
        .map_err(|e| anyhow::anyhow!(e))?
        .as_deref()
        == Some("1");

    let kv: BTreeMap<String, String> = data::list_kv(&conn)
        .map_err(|e| anyhow::anyhow!(e))?
        .into_iter()
        .map(|(k, j, _)| (k, j))
        .collect();

    let (mut s, t) = merge::merge_from_kv(cfg, kv).map_err(|e| anyhow::anyhow!(e))?;
    validate::validate_snapshot(&s, &t)
        .map_err(|e| anyhow::anyhow!("settings snapshot invalid: {e}"))?;
    validate::apply_recovery(&mut s, recovery_pending);

    let db = Some(Arc::new(Mutex::new(conn)));
    Ok(RuntimeConfigInit {
        effective_safety: s,
        effective_telemetry: t,
        recovery_pending,
        db,
    })
}

use anyhow::Context;

/// Try to open. On failure, quarantine, recreate, re-seed, set `recovery_pending=1` in `meta`.
fn open_or_repair(path: &Path, cfg: &Config) -> Result<(Connection, bool)> {
    match data::open(path) {
        Ok(c) => Ok((c, false)),
        Err(e) => {
            warn!(error = %e, path = %path.display(), "runtime db failed; quarantine and re-seed");
            let _b = data::quarantine_corrupt(path)?;
            let mut c = data::open(path).context("re-open after quarantine")?;
            c.execute("DELETE FROM settings_kv", [])
                .map_err(|e| anyhow::anyhow!(e))?;
            c.execute("DELETE FROM meta", [])
                .map_err(|e| anyhow::anyhow!(e))?;
            c.execute("DELETE FROM inventory_doc", [])
                .map_err(|e| anyhow::anyhow!(e))?;
            seed_all(&mut c, cfg).map_err(|e| anyhow::anyhow!(e))?;
            data::set_meta(&c, META_SEEDED, "1").map_err(|e| anyhow::anyhow!(e))?;
            data::set_meta(&c, META_RECOVERY, "1").map_err(|e| anyhow::anyhow!(e))?;
            Ok((c, true))
        }
    }
}
