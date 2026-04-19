//! REST surface for the Logs page.
//!
//! - `GET    /api/logs`         — paginated history query (newest first).
//! - `DELETE /api/logs`         — clear the entire log store.
//! - `GET    /api/logs/level`   — current `EnvFilter` snapshot.
//! - `PUT    /api/logs/level`   — swap the filter at runtime + persist
//!   the new directive string to `.rudyd/log_filter.txt` so it survives
//!   a daemon restart.
//!
//! The store reads run through `tokio::task::spawn_blocking` (see
//! `log_store.rs`) so SQLite's blocking C API doesn't tie up the runtime.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

use crate::audit::{AuditEntry, AuditResult};
use crate::log_store::LogFilter;
use crate::state::SharedState;
use crate::types::{ApiError, LogEntry, LogFilterDirective, LogFilterState, LogLevel, LogSource};

/// Path next to `audit.jsonl` where we persist the latest accepted
/// filter directive string. Survives daemon restarts; on next boot
/// `main.rs` reads it and overrides the config default if present.
pub const LOG_FILTER_FILE_NAME: &str = "log_filter.txt";

#[derive(Debug, Clone, Deserialize)]
pub struct ListQuery {
    /// Comma-separated list, e.g. `"warn,error"`. Missing ≡ all levels.
    pub level: Option<String>,
    /// Comma-separated list of `tracing|audit`. Missing ≡ both.
    pub source: Option<String>,
    pub q: Option<String>,
    pub target: Option<String>,
    pub since_ms: Option<i64>,
    pub before_id: Option<i64>,
    /// Default 200, cap 1000.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListResponse {
    pub entries: Vec<LogEntry>,
    /// Pass back as `before_id` to fetch the next (older) page; `null`
    /// when there's no more history.
    pub next_before_id: Option<i64>,
}

pub async fn list(
    State(state): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ListResponse>, (StatusCode, Json<ApiError>)> {
    let store = state.log_store.get().ok_or_else(store_unavailable_error)?;

    let levels = parse_levels(q.level.as_deref());
    let sources = parse_sources(q.source.as_deref());
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);

    let filter = LogFilter {
        levels,
        sources,
        q: q.q,
        target: q.target,
        since_ms: q.since_ms,
        before_id: q.before_id,
        limit,
    };

    let entries = store.query(filter).await.map_err(|e| {
        tracing::error!("logs query failed: {e:#}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "log_query_failed".into(),
                detail: Some(format!("{e:#}")),
            }),
        )
    })?;

    let next_before_id = if entries.len() == limit {
        entries.last().map(|e| e.id)
    } else {
        None
    };

    Ok(Json(ListResponse {
        entries,
        next_before_id,
    }))
}

pub async fn clear(
    State(state): State<SharedState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let store = state.log_store.get().ok_or_else(store_unavailable_error)?;

    store.clear().await.map_err(|e| {
        tracing::error!("logs clear failed: {e:#}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "log_clear_failed".into(),
                detail: Some(format!("{e:#}")),
            }),
        )
    })?;

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "logs_clear".into(),
        target: None,
        details: serde_json::json!({}),
        result: AuditResult::Ok,
    });

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn get_level(State(state): State<SharedState>) -> Json<LogFilterState> {
    Json(snapshot_filter(&state))
}

#[derive(Debug, Clone, Deserialize)]
pub struct PutLevelBody {
    pub raw: String,
}

pub async fn put_level(
    State(state): State<SharedState>,
    Json(body): Json<PutLevelBody>,
) -> Result<Json<LogFilterState>, (StatusCode, Json<ApiError>)> {
    let raw = body.raw.trim();
    if raw.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "empty_filter".into(),
                detail: Some("raw filter directive must be non-empty".into()),
            }),
        ));
    }

    // Validate by parsing. `EnvFilter::try_new` mirrors what the runtime
    // installs; if it parses here it parses inside the reload handle too.
    let parsed = EnvFilter::try_new(raw).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "invalid_filter".into(),
                detail: Some(format!("{e}")),
            }),
        )
    })?;

    let setter = state.filter_reload.get().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiError {
                error: "filter_reload_unavailable".into(),
                detail: Some(
                    "the daemon was started without a runtime-mutable filter (test build?)".into(),
                ),
            }),
        )
    })?;

    setter(parsed).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "filter_reload_failed".into(),
                detail: Some(e),
            }),
        )
    })?;

    if let Err(e) = persist_filter(&state, raw) {
        // Persistence failure isn't fatal — the filter was applied — but
        // log it loudly so an operator notices that the choice won't
        // survive a restart.
        tracing::warn!("logs: failed to persist filter to disk: {e:#}");
    }

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "logs_level_set".into(),
        target: None,
        details: serde_json::json!({ "raw": raw }),
        result: AuditResult::Ok,
    });

    Ok(Json(snapshot_filter_with(raw)))
}

fn snapshot_filter(state: &SharedState) -> LogFilterState {
    // Prefer the persisted file if present; otherwise fall back to
    // config default. This is the same precedence `main.rs` applies on
    // boot, so the snapshot matches what the operator would see if they
    // restarted right now.
    let raw = persisted_filter(state).unwrap_or_else(|| state.cfg.logs.default_filter.clone());
    snapshot_filter_with(&raw)
}

fn snapshot_filter_with(raw: &str) -> LogFilterState {
    let (default, directives) = parse_directives(raw);
    LogFilterState {
        default,
        directives,
        raw: raw.to_string(),
    }
}

fn persist_filter(state: &SharedState, raw: &str) -> std::io::Result<()> {
    let path = filter_persist_path(state);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, raw)
}

fn persisted_filter(state: &SharedState) -> Option<String> {
    let path = filter_persist_path(state);
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn filter_persist_path(state: &SharedState) -> std::path::PathBuf {
    state
        .cfg
        .paths
        .audit_log
        .parent()
        .map(|p| p.join(LOG_FILTER_FILE_NAME))
        .unwrap_or_else(|| std::path::PathBuf::from(LOG_FILTER_FILE_NAME))
}

fn parse_directives(raw: &str) -> (LogLevel, Vec<LogFilterDirective>) {
    let mut default = LogLevel::Info;
    let mut directives: Vec<LogFilterDirective> = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match part.split_once('=') {
            None => {
                if let Some(lv) = LogLevel::from_str_loose(part) {
                    default = lv;
                }
            }
            Some((target, level_str)) => {
                if let Some(level) = LogLevel::from_str_loose(level_str.trim()) {
                    directives.push(LogFilterDirective {
                        target: Some(target.trim().to_string()),
                        level,
                    });
                }
            }
        }
    }
    (default, directives)
}

fn parse_levels(raw: Option<&str>) -> Vec<LogLevel> {
    raw.map(|s| {
        s.split(',')
            .filter_map(|p| LogLevel::from_str_loose(p.trim()))
            .collect()
    })
    .unwrap_or_default()
}

fn parse_sources(raw: Option<&str>) -> Vec<LogSource> {
    raw.map(|s| {
        s.split(',')
            .filter_map(|p| match p.trim().to_ascii_lowercase().as_str() {
                "tracing" => Some(LogSource::Tracing),
                "audit" => Some(LogSource::Audit),
                _ => None,
            })
            .collect()
    })
    .unwrap_or_default()
}

fn store_unavailable_error() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ApiError {
            error: "log_store_unavailable".into(),
            detail: Some("daemon started without a log store".into()),
        }),
    )
}
