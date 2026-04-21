//! Persistent log store wire types (SQLite + SPA Logs page).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Severity level for a captured log entry. Mirrors `tracing::Level`'s
/// 5-level vocabulary; the daemon stores it as a small integer in SQLite
/// (`trace=0 .. error=4`) and exposes the string form to the SPA so JSON
/// reads stay readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_i64(self) -> i64 {
        match self {
            LogLevel::Trace => 0,
            LogLevel::Debug => 1,
            LogLevel::Info => 2,
            LogLevel::Warn => 3,
            LogLevel::Error => 4,
        }
    }

    pub fn from_i64(v: i64) -> LogLevel {
        match v {
            0 => LogLevel::Trace,
            1 => LogLevel::Debug,
            3 => LogLevel::Warn,
            4 => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }

    pub fn from_str_loose(s: &str) -> Option<LogLevel> {
        match s.to_ascii_lowercase().as_str() {
            "trace" => Some(LogLevel::Trace),
            "debug" => Some(LogLevel::Debug),
            "info" => Some(LogLevel::Info),
            "warn" | "warning" => Some(LogLevel::Warn),
            "error" => Some(LogLevel::Error),
            _ => None,
        }
    }
}

/// Where a log entry came from. `Tracing` is anything that flowed through
/// the tracing subscriber (cortex internals, axum traces, etc.); `Audit`
/// is the operator-action stream that also lands in `audit.jsonl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    Tracing,
    Audit,
}

impl LogSource {
    pub fn as_i64(self) -> i64 {
        match self {
            LogSource::Tracing => 0,
            LogSource::Audit => 1,
        }
    }

    pub fn from_i64(v: i64) -> LogSource {
        match v {
            1 => LogSource::Audit,
            _ => LogSource::Tracing,
        }
    }
}

/// One captured log entry — the unit the SPA Logs page paginates and tails.
///
/// Layout matches the SQLite schema in [`crate::log_store`]; `id` is the
/// primary key and the keyset cursor for pagination. `audit_*` and
/// `session_id` / `remote` are populated only for `source = Audit` rows.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct LogEntry {
    pub id: i64,
    pub t_ms: i64,
    pub level: LogLevel,
    pub source: LogSource,
    pub target: String,
    pub message: String,
    /// Free-form key/value bag (tracing fields or audit details). Always
    /// an object shape on the wire; missing-or-empty serializes as `{}`.
    pub fields: serde_json::Value,
    /// Span path, e.g. `motion::sweep:tick`. `None` outside any span.
    pub span: Option<String>,
    pub action: Option<String>,
    pub audit_target: Option<String>,
    pub result: Option<String>,
    pub session_id: Option<String>,
    pub remote: Option<String>,
}

/// One parsed `EnvFilter` directive. `target = None` means the bare
/// default level (`info` in `info,cortex=debug`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct LogFilterDirective {
    pub target: Option<String>,
    pub level: LogLevel,
}

/// Snapshot of the runtime tracing-filter state. `raw` round-trips through
/// `EnvFilter::try_new` so the SPA's "Advanced" textarea reads back exactly
/// what the daemon parses.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct LogFilterState {
    pub default: LogLevel,
    pub directives: Vec<LogFilterDirective>,
    pub raw: String,
}
