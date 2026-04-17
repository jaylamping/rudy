//! Append-only JSONL audit log.
//!
//! Every mutating REST request and every WebTransport session lifecycle event
//! is recorded. Survives restarts; operator rotates with logrotate.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct AuditLog {
    path: PathBuf,
    file: Mutex<File>,
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
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self, entry: AuditEntry) {
        if let Ok(mut line) = serde_json::to_vec(&entry) {
            line.push(b'\n');
            if let Ok(mut f) = self.file.lock() {
                let _ = f.write_all(&line);
                let _ = f.flush();
            }
        }
    }
}
