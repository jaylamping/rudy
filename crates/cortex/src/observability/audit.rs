//! Append-only JSONL audit log.
//!
//! Every mutating REST request and every WebTransport session lifecycle event
//! is recorded. Survives restarts; operator rotates with logrotate.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::log_store::LogStore;
use crate::types::{LogEntry, LogLevel, LogSource};

/// Sinks that audit entries fan out to in addition to the on-disk JSONL.
///
/// Wired by `main.rs` once the `LogStore` and live broadcast exist; until
/// then `AuditLog::write` only appends to disk. This ordering avoids a
/// chicken-and-egg dance during startup (audit log is opened before the
/// log store).
#[derive(Debug)]
pub struct AuditFanout {
    pub store: LogStore,
    pub live_tx: broadcast::Sender<LogEntry>,
}

#[derive(Debug)]
pub struct AuditLog {
    #[allow(dead_code)]
    path: PathBuf,
    file: Mutex<File>,
    /// Lazily populated after `LogStore` is constructed in `main.rs`. Set
    /// exactly once via [`AuditLog::attach_fanout`]; subsequent calls are
    /// no-ops (the daemon never re-opens its store).
    fanout: OnceLock<AuditFanout>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub session_id: Option<String>,
    pub remote: Option<String>,
    pub action: String,
    pub target: Option<String>,
    pub details: serde_json::Value,
    pub result: AuditResult,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    Ok,
    Denied,
    Error,
}

impl AuditLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating audit log parent {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening audit log {}", path.display()))?;
        Ok(Self {
            path,
            file: Mutex::new(file),
            fanout: OnceLock::new(),
        })
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Wire the unified log store + live broadcast. Called once from
    /// `main.rs` after both exist. Idempotent: extra calls are silently
    /// ignored so the test harness can attach in any order.
    pub fn attach_fanout(&self, fanout: AuditFanout) {
        let _ = self.fanout.set(fanout);
    }

    pub fn write(&self, entry: AuditEntry) {
        if let Ok(mut line) = serde_json::to_vec(&entry) {
            line.push(b'\n');
            if let Ok(mut f) = self.file.lock() {
                let _ = f.write_all(&line);
                let _ = f.flush();
            }
        }

        if let Some(fanout) = self.fanout.get() {
            let log_entry = audit_to_log_entry(&entry);
            fanout.store.submit(log_entry.clone());
            let _ = fanout.live_tx.send(log_entry);
        }
    }
}

fn audit_to_log_entry(a: &AuditEntry) -> LogEntry {
    let level = match a.result {
        AuditResult::Ok => LogLevel::Info,
        AuditResult::Denied => LogLevel::Warn,
        AuditResult::Error => LogLevel::Error,
    };
    let result_str = match a.result {
        AuditResult::Ok => "ok",
        AuditResult::Denied => "denied",
        AuditResult::Error => "error",
    };
    LogEntry {
        id: 0,
        t_ms: a.timestamp.timestamp_millis(),
        level,
        source: LogSource::Audit,
        target: "audit".to_string(),
        message: a.action.clone(),
        fields: a.details.clone(),
        span: None,
        action: Some(a.action.clone()),
        audit_target: a.target.clone(),
        result: Some(result_str.to_string()),
        session_id: a.session_id.clone(),
        remote: a.remote.clone(),
    }
}
