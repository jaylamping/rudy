//! SQLite: settings key/value, `meta`, optional inventory body.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

const SCHEMA_VERSION: i32 = 1;

/// Open a DB, create parent dirs, run migrations, and verify integrity.
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create runtime db parent {}", parent.display()))?;
        }
    }
    let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("pragma journal_mode=WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .context("pragma synchronous=NORMAL")?;
    migrate(&conn)?;
    let ok: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .context("PRAGMA integrity_check")?;
    if ok != "ok" {
        anyhow::bail!("integrity_check: {ok}");
    }
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    let ver: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);
    if ver < SCHEMA_VERSION {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
                k TEXT PRIMARY KEY,
                v TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings_kv (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS inventory_doc (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                body TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            "#,
        )
        .context("runtime db schema create")?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .context("set user_version")?;
    }
    Ok(())
}

pub fn get_meta(conn: &Connection, k: &str) -> Result<Option<String>> {
    let v = conn
        .query_row("SELECT v FROM meta WHERE k = ?1", params![k], |r| r.get(0))
        .optional()?;
    Ok(v)
}

/// `k` values whose key starts with `prefix` (e.g. `profile:` for saved profiles).
pub fn list_meta_with_prefix(conn: &Connection, prefix: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn
        .prepare("SELECT k, v FROM meta WHERE k LIKE ?1 ORDER BY k")
        .context("prepare list meta")?;
    let pat = format!("{prefix}%");
    let mut rows = stmt.query(params![pat]).context("list meta query")?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().context("meta row")? {
        out.push((row.get(0).context("k")?, row.get(1).context("v")?));
    }
    Ok(out)
}

pub fn delete_meta(conn: &Connection, k: &str) -> Result<()> {
    conn.execute("DELETE FROM meta WHERE k = ?1", params![k])
        .context("delete meta")?;
    Ok(())
}

pub fn set_meta(conn: &Connection, k: &str, v: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (k, v) VALUES (?1, ?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
        params![k, v],
    )
    .context("set meta")?;
    Ok(())
}

pub fn list_kv(conn: &Connection) -> Result<Vec<(String, String, i64)>> {
    let mut stmt = conn
        .prepare("SELECT key, value_json, updated_at_ms FROM settings_kv ORDER BY key")
        .context("prepare list_kv")?;
    let mut rows = stmt.query([]).context("query list_kv")?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().context("next row")? {
        out.push((
            row.get(0).context("key")?,
            row.get(1).context("value_json")?,
            row.get(2).context("updated_at")?,
        ));
    }
    Ok(out)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Replace all settings in one transaction.
pub fn replace_all_kv(
    conn: &mut Connection,
    entries: &[(String, serde_json::Value)],
) -> Result<()> {
    let tx = conn.transaction().context("tx begin")?;
    tx.execute("DELETE FROM settings_kv", [])
        .context("delete settings")?;
    for (k, v) in entries {
        let json = v.to_string();
        tx.execute(
            "INSERT INTO settings_kv (key, value_json, updated_at_ms) VALUES (?1, ?2, ?3)",
            params![k, json, now_ms()],
        )
        .with_context(|| format!("insert {k}"))?;
    }
    tx.commit().context("tx commit")?;
    Ok(())
}

pub fn upsert_kv(conn: &Connection, key: &str, value: &serde_json::Value) -> Result<()> {
    let json = value.to_string();
    conn.execute(
        "INSERT INTO settings_kv (key, value_json, updated_at_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at_ms = excluded.updated_at_ms",
        params![key, json, now_ms()],
    )
    .with_context(|| format!("upsert {key}"))?;
    Ok(())
}

pub fn get_inventory_json(conn: &Connection) -> Result<Option<String>> {
    let r = conn
        .query_row("SELECT body FROM inventory_doc WHERE id = 1", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()?;
    Ok(r)
}

pub fn set_inventory_json(conn: &Connection, body: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO inventory_doc (id, body, updated_at_ms) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET body = excluded.body, updated_at_ms = excluded.updated_at_ms",
        params![body, now_ms()],
    )
    .context("set inventory_doc")?;
    Ok(())
}

/// Move bad DB aside so we can recreate.
pub fn quarantine_corrupt(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let mut backup = path.to_path_buf();
    let new_name = path
        .file_name()
        .map(|n| format!("{}.corrupt.{}", n.to_string_lossy(), ts))
        .unwrap_or_else(|| format!("db.corrupt.{ts}"));
    backup.set_file_name(new_name);
    fs::rename(path, &backup)
        .with_context(|| format!("quarantine {} -> {}", path.display(), backup.display()))?;
    Ok(backup)
}
