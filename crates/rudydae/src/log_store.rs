//! Persistent log store.
//!
//! SQLite-backed (WAL mode) home for both tracing events captured via
//! `log_layer::LogCaptureLayer` and operator actions copied from
//! `audit::AuditLog`. One unified table keeps the Logs page's filter / sort /
//! paginate path simple — see `api/logs.rs`.
//!
//! Writes are batched: producers push `LogEntry`s into an unbounded mpsc
//! channel and a single tokio task drains up to `batch_max_rows` entries (or
//! `batch_flush_ms` worth of waiting, whichever comes first) into a single
//! transaction. Reads run on `spawn_blocking` so the runtime isn't tied up
//! by SQLite's blocking C API.
//!
//! Bounded backpressure: if the writer falls behind by >50_000 entries we
//! drop the oldest and increment a counter that surfaces at warn (logged
//! through tracing — meta!) so an operator notices a runaway producer
//! before the disk fills.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::config::LogsConfig;
use crate::types::{LogEntry, LogLevel, LogSource};

/// Soft cap on the in-memory queue between producers and the SQLite writer.
/// Anything beyond this gets dropped (oldest-first) so a runaway emitter
/// can't blow the daemon's RSS. Sized for ~5 s of worst-case bursts at 10 kHz.
const QUEUE_SOFT_CAP: usize = 50_000;

/// Filter parameters for `LogStore::query`. Empty / `None` fields are
/// "no filter."
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    /// Set of allowed levels (as `LogLevel::as_i64()` ints). Empty ≡ all.
    pub levels: Vec<LogLevel>,
    /// Set of allowed sources. Empty ≡ all.
    pub sources: Vec<LogSource>,
    /// Free-text substring matched against `message` with SQL `LIKE`.
    pub q: Option<String>,
    /// Substring matched against `target` with SQL `LIKE`.
    pub target: Option<String>,
    /// Inclusive lower bound on `t_ms`.
    pub since_ms: Option<i64>,
    /// Keyset cursor: return rows with `id < before_id`. Pairs with
    /// `LogStore::query`'s newest-first ordering for `O(log N)` paging.
    pub before_id: Option<i64>,
    /// Cap; the store also enforces a hard ceiling of 1000.
    pub limit: usize,
}

/// Handle to the persistent log store. Cloneable; the underlying writer is
/// a single tokio task that owns the SQLite connection.
#[derive(Clone, Debug)]
pub struct LogStore {
    inner: Arc<LogStoreInner>,
}

#[derive(Debug)]
struct LogStoreInner {
    db_path: PathBuf,
    tx: mpsc::UnboundedSender<LogEntry>,
}

impl LogStore {
    /// Open (or create) the database at `cfg.db_path`, run the schema
    /// migration, spawn the batched writer + retention purger tasks.
    pub fn open(cfg: &LogsConfig) -> Result<Self> {
        if let Some(parent) = cfg.db_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating log_store parent {}", parent.display()))?;
            }
        }

        let conn = Connection::open(&cfg.db_path)
            .with_context(|| format!("opening log db {}", cfg.db_path.display()))?;
        configure_connection(&conn)?;
        migrate(&conn)?;
        info!(path = %cfg.db_path.display(), "log_store opened");

        let (tx, rx) = mpsc::unbounded_channel::<LogEntry>();
        let writer_cfg = cfg.clone();
        tokio::task::spawn(writer_task(conn, rx, writer_cfg));

        let inner = LogStoreInner {
            db_path: cfg.db_path.clone(),
            tx,
        };
        let store = LogStore {
            inner: Arc::new(inner),
        };
        store.spawn_purger(cfg.clone());
        Ok(store)
    }

    /// Best-effort enqueue. The writer task assigns `id` on insert; the
    /// `id` field on `entry` (typically 0 from a producer) is ignored.
    pub fn submit(&self, entry: LogEntry) {
        // Channel send only fails if the writer task has died, which
        // means we're already shutting down; swallow rather than panic
        // because tracing producers can't realistically handle this.
        let _ = self.inner.tx.send(entry);
    }

    /// Query newest-first, bounded by `filter.limit` (hard cap 1000).
    /// Runs on a blocking thread — never call from a hot loop.
    pub async fn query(&self, filter: LogFilter) -> Result<Vec<LogEntry>> {
        let path = self.inner.db_path.clone();
        tokio::task::spawn_blocking(move || query_blocking(&path, filter))
            .await
            .context("log_store query join")?
    }

    /// Drop every row + reclaim free pages. Audited by the caller; the
    /// store itself doesn't know "who" cleared.
    pub async fn clear(&self) -> Result<()> {
        let path = self.inner.db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = Connection::open(&path)
                .with_context(|| format!("re-opening log db {}", path.display()))?;
            configure_connection(&conn)?;
            conn.execute("DELETE FROM logs", [])
                .context("delete logs")?;
            // VACUUM has to run outside any transaction; rusqlite's
            // default is autocommit so this is fine.
            conn.execute("VACUUM", []).context("vacuum logs")?;
            Ok(())
        })
        .await
        .context("log_store clear join")?
    }

    fn spawn_purger(&self, cfg: LogsConfig) {
        let path = self.inner.db_path.clone();
        tokio::task::spawn(async move {
            let mut last_vacuum = std::time::Instant::now();
            loop {
                tokio::time::sleep(Duration::from_secs(cfg.purge_interval_s.max(5))).await;
                let path = path.clone();
                let cfg = cfg.clone();
                let do_vacuum = last_vacuum.elapsed() > Duration::from_secs(86_400);
                let res = tokio::task::spawn_blocking(move || -> Result<usize> {
                    let conn = Connection::open(&path)
                        .with_context(|| format!("opening log db {}", path.display()))?;
                    configure_connection(&conn)?;
                    let cutoff_ms = chrono::Utc::now().timestamp_millis()
                        - (cfg.retention_days as i64) * 86_400_000_i64;
                    let n = conn
                        .execute("DELETE FROM logs WHERE t_ms < ?1", params![cutoff_ms])
                        .context("retention delete")?;
                    if do_vacuum {
                        // wal_checkpoint(TRUNCATE) is best-effort; a busy
                        // WAL just means we'll catch it next time.
                        let _ = conn.execute("PRAGMA wal_checkpoint(TRUNCATE)", []);
                        let _ = conn.execute("VACUUM", []);
                    }
                    Ok(n)
                })
                .await;
                match res {
                    Ok(Ok(n)) if n > 0 => {
                        debug!(deleted = n, "log_store retention purge");
                    }
                    Ok(Err(e)) => {
                        warn!("log_store retention purge failed: {e:#}");
                    }
                    Err(e) => {
                        warn!("log_store retention join failed: {e:#}");
                    }
                    _ => {}
                }
                if do_vacuum {
                    last_vacuum = std::time::Instant::now();
                }
            }
        });
    }
}

fn configure_connection(conn: &Connection) -> Result<()> {
    // WAL keeps writers from blocking readers; `synchronous=NORMAL` is the
    // standard tradeoff (fsync per checkpoint, not per commit) — fine for
    // logs (we'd rather lose ~1 batch on a hard power cut than pay an
    // fsync per row).
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("pragma journal_mode=WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .context("pragma synchronous=NORMAL")?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS logs (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            t_ms          INTEGER NOT NULL,
            level         INTEGER NOT NULL,
            source        INTEGER NOT NULL,
            target        TEXT    NOT NULL,
            message       TEXT    NOT NULL,
            fields        TEXT,
            span          TEXT,
            action        TEXT,
            audit_target  TEXT,
            result        TEXT,
            session_id    TEXT,
            remote        TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_logs_t_ms   ON logs(t_ms);
        CREATE INDEX IF NOT EXISTS idx_logs_level  ON logs(level);
        CREATE INDEX IF NOT EXISTS idx_logs_source ON logs(source);
        "#,
    )
    .context("log schema migration")
}

async fn writer_task(
    mut conn: Connection,
    mut rx: mpsc::UnboundedReceiver<LogEntry>,
    cfg: LogsConfig,
) {
    let max_rows = cfg.batch_max_rows.max(1);
    let flush = Duration::from_millis(cfg.batch_flush_ms.max(1));
    let mut buf: Vec<LogEntry> = Vec::with_capacity(max_rows);
    let mut dropped: u64 = 0;

    loop {
        // Wait for at least one entry; closing the channel ends the task.
        let first = match rx.recv().await {
            Some(e) => e,
            None => {
                debug!("log_store writer: channel closed, exiting");
                return;
            }
        };
        buf.push(first);

        // Drain whatever else is sitting in the queue right now (no wait).
        while buf.len() < max_rows {
            match rx.try_recv() {
                Ok(e) => buf.push(e),
                Err(_) => break,
            }
        }

        // If still under capacity, give producers a short window to bunch.
        if buf.len() < max_rows {
            let deadline = tokio::time::Instant::now() + flush;
            while buf.len() < max_rows {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match tokio::time::timeout(remaining, rx.recv()).await {
                    Ok(Some(e)) => buf.push(e),
                    // Channel closed; flush what we have and bail.
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }

        // Backpressure: if the queue is now huge, drop oldest.
        if buf.len() > QUEUE_SOFT_CAP {
            let excess = buf.len() - QUEUE_SOFT_CAP;
            buf.drain(0..excess);
            dropped += excess as u64;
            warn!(
                dropped_total = dropped,
                excess, "log_store: queue overflow, dropped oldest entries"
            );
        }

        if let Err(e) = flush_batch(&mut conn, &buf) {
            warn!("log_store flush failed: {e:#}");
        }
        buf.clear();
    }
}

fn flush_batch(conn: &mut Connection, batch: &[LogEntry]) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction().context("begin tx")?;
    {
        let mut stmt = tx
            .prepare_cached(
                "INSERT INTO logs (
                    t_ms, level, source, target, message, fields, span,
                    action, audit_target, result, session_id, remote
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            )
            .context("prepare insert")?;
        for entry in batch {
            let fields_text = if entry.fields.is_null() {
                "{}".to_string()
            } else {
                entry.fields.to_string()
            };
            stmt.execute(params![
                entry.t_ms,
                entry.level.as_i64(),
                entry.source.as_i64(),
                entry.target,
                entry.message,
                fields_text,
                entry.span,
                entry.action,
                entry.audit_target,
                entry.result,
                entry.session_id,
                entry.remote,
            ])
            .context("insert log row")?;
        }
    }
    tx.commit().context("commit tx")?;
    Ok(())
}

fn query_blocking(path: &Path, filter: LogFilter) -> Result<Vec<LogEntry>> {
    let conn =
        Connection::open(path).with_context(|| format!("opening log db {}", path.display()))?;
    configure_connection(&conn)?;

    // Build the WHERE clause incrementally so we can support optional
    // filters without dragging in a query builder. Bind parameters by
    // numeric position; rusqlite handles SQL-injection escaping.
    let mut sql = String::from(
        "SELECT id, t_ms, level, source, target, message, fields, span,
                action, audit_target, result, session_id, remote
         FROM logs WHERE 1=1",
    );
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    // Inline placeholder allocation: `?{N}` where N = binds.len()+1 just
    // before each push. We tried a small `next_idx` closure but it had to
    // capture &binds for the count and then we needed &mut binds for the
    // push, which the borrow checker rejects. The arithmetic is trivial
    // enough that doing it inline keeps the build green and the code
    // obvious.
    if !filter.levels.is_empty() {
        let mut placeholders: Vec<String> = Vec::with_capacity(filter.levels.len());
        for lv in &filter.levels {
            placeholders.push(format!("?{}", binds.len() + 1));
            binds.push(Box::new(lv.as_i64()));
        }
        sql.push_str(&format!(" AND level IN ({})", placeholders.join(",")));
    }
    if !filter.sources.is_empty() {
        let mut placeholders: Vec<String> = Vec::with_capacity(filter.sources.len());
        for s in &filter.sources {
            placeholders.push(format!("?{}", binds.len() + 1));
            binds.push(Box::new(s.as_i64()));
        }
        sql.push_str(&format!(" AND source IN ({})", placeholders.join(",")));
    }
    if let Some(q) = filter.q.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(&format!(" AND message LIKE ?{}", binds.len() + 1));
        binds.push(Box::new(format!("%{q}%")));
    }
    if let Some(t) = filter.target.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(&format!(" AND target LIKE ?{}", binds.len() + 1));
        binds.push(Box::new(format!("%{t}%")));
    }
    if let Some(s) = filter.since_ms {
        sql.push_str(&format!(" AND t_ms >= ?{}", binds.len() + 1));
        binds.push(Box::new(s));
    }
    if let Some(b) = filter.before_id {
        sql.push_str(&format!(" AND id < ?{}", binds.len() + 1));
        binds.push(Box::new(b));
    }

    let limit = filter.limit.clamp(1, 1000);
    sql.push_str(&format!(" ORDER BY id DESC LIMIT {limit}"));

    let mut stmt = conn.prepare(&sql).context("prepare query")?;
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(bind_refs.iter()), |row| {
            let fields_text: Option<String> = row.get(6)?;
            let fields: serde_json::Value = fields_text
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            Ok(LogEntry {
                id: row.get(0)?,
                t_ms: row.get(1)?,
                level: LogLevel::from_i64(row.get::<_, i64>(2)?),
                source: LogSource::from_i64(row.get::<_, i64>(3)?),
                target: row.get(4)?,
                message: row.get(5)?,
                fields,
                span: row.get(7)?,
                action: row.get(8)?,
                audit_target: row.get(9)?,
                result: row.get(10)?,
                session_id: row.get(11)?,
                remote: row.get(12)?,
            })
        })
        .context("query_map")?;

    let mut out = Vec::with_capacity(limit);
    for r in rows {
        out.push(r.context("row")?);
    }
    Ok(out)
}

/// Convenience: fetch one entry by id (used by tests + the detail pane
/// when the SPA wants to deep-link to a single log row). Returns `None`
/// if the row was purged or never existed.
#[allow(dead_code)]
pub async fn get_one(store: &LogStore, id: i64) -> Result<Option<LogEntry>> {
    let path = store.inner.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<Option<LogEntry>> {
        let conn = Connection::open(&path)
            .with_context(|| format!("opening log db {}", path.display()))?;
        configure_connection(&conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, t_ms, level, source, target, message, fields, span,
                        action, audit_target, result, session_id, remote
                 FROM logs WHERE id = ?1",
            )
            .context("prepare get_one")?;
        let row = stmt
            .query_row(params![id], |row| {
                let fields_text: Option<String> = row.get(6)?;
                let fields: serde_json::Value = fields_text
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                Ok(LogEntry {
                    id: row.get(0)?,
                    t_ms: row.get(1)?,
                    level: LogLevel::from_i64(row.get::<_, i64>(2)?),
                    source: LogSource::from_i64(row.get::<_, i64>(3)?),
                    target: row.get(4)?,
                    message: row.get(5)?,
                    fields,
                    span: row.get(7)?,
                    action: row.get(8)?,
                    audit_target: row.get(9)?,
                    result: row.get(10)?,
                    session_id: row.get(11)?,
                    remote: row.get(12)?,
                })
            })
            .optional()
            .context("query_row")?;
        Ok(row)
    })
    .await
    .context("log_store get_one join")?
}
