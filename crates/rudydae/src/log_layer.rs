//! tracing `Layer` that captures every event into the persistent store and
//! the live broadcast channel.
//!
//! Pairs with `tracing_subscriber::fmt::layer()` (which keeps the existing
//! stdout / journald output untouched) and with the reloadable `EnvFilter`
//! installed in `main.rs`. The same per-event level filter applied to fmt
//! also gates this layer because it sits *after* the filter in the layer
//! stack.
//!
//! The capture is intentionally cheap on the hot path: we walk the event's
//! fields once into a `serde_json::Map`, hand the resulting `LogEntry` to a
//! non-blocking mpsc (the store) and a non-blocking broadcast (the live
//! tail), and return. Heavy work — JSON-encoding fields for SQLite, the
//! actual SQLite insert — runs on the writer task in `log_store.rs`.

use std::fmt::Write as _;

use serde_json::Map as JsonMap;
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::span::Attributes;
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::log_store::LogStore;
use crate::types::{LogEntry, LogLevel, LogSource};

/// Layer that captures events into [`LogStore`] + a [`broadcast::Sender`].
///
/// `live_tx` is the SAME sender the WT router subscribes to per session,
/// so a captured event lights up both the persistent history (via the
/// store) and every connected operator's Logs page in one shot.
pub struct LogCaptureLayer {
    store: LogStore,
    live_tx: broadcast::Sender<LogEntry>,
}

impl LogCaptureLayer {
    pub fn new(store: LogStore, live_tx: broadcast::Sender<LogEntry>) -> Self {
        Self { store, live_tx }
    }
}

impl<S> Layer<S> for LogCaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level = level_from_tracing(*metadata.level());

        // Visit fields once; pull `message` out into the entry's `message`
        // and the rest into a JSON object on the side. Mirrors what the
        // fmt layer does for stdout.
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let message = visitor.message.unwrap_or_default();
        let fields_value = if visitor.fields.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::Value::Object(visitor.fields)
        };

        // Walk the current span list to build a `parent::child:leaf` path.
        // Useful for filtering in the SPA without re-deriving from `target`.
        let span_path = build_span_path(&ctx, event);

        let entry = LogEntry {
            id: 0,
            t_ms: chrono::Utc::now().timestamp_millis(),
            level,
            source: LogSource::Tracing,
            target: metadata.target().to_string(),
            message,
            fields: fields_value,
            span: span_path,
            action: None,
            audit_target: None,
            result: None,
            session_id: None,
            remote: None,
        };

        // Best-effort fan-out. Both sinks drop on overflow / closed
        // rather than blocking the producer.
        self.store.submit(entry.clone());
        let _ = self.live_tx.send(entry);
    }

    /// Cache the span name on `new_span` so `build_span_path` doesn't have
    /// to re-derive it on every event. The `Registry` already stores the
    /// metadata; this is just the hook to opt in.
    fn on_new_span(&self, _attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        // No extension data needed; LookupSpan + metadata().name() already
        // gets us what we want in build_span_path. This impl exists so a
        // future enhancement (e.g. capturing span fields) has an obvious
        // place to land.
    }
}

fn level_from_tracing(lv: tracing::Level) -> LogLevel {
    match lv {
        tracing::Level::TRACE => LogLevel::Trace,
        tracing::Level::DEBUG => LogLevel::Debug,
        tracing::Level::INFO => LogLevel::Info,
        tracing::Level::WARN => LogLevel::Warn,
        tracing::Level::ERROR => LogLevel::Error,
    }
}

fn build_span_path<S>(ctx: &Context<'_, S>, event: &Event<'_>) -> Option<String>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let scope = ctx.event_scope(event)?;
    let mut parts: Vec<&str> = Vec::new();
    for span in scope.from_root() {
        parts.push(span.metadata().name());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(":"))
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    fields: JsonMap<String, serde_json::Value>,
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::Bool(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        let mut s = String::new();
        let _ = write!(&mut s, "{value}");
        if field.name() == "message" {
            self.message = Some(s);
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(s));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // `tracing::info!("hello world")` lands here as field "message"
        // with the formatted string — same shape `fmt::layer` uses.
        let mut s = String::new();
        let _ = write!(&mut s, "{value:?}");
        // Strip surrounding quotes for the common `Debug` of a `&str`.
        let trimmed = if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
            s[1..s.len() - 1].to_string()
        } else {
            s
        };
        if field.name() == "message" {
            self.message = Some(trimmed);
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(trimmed));
        }
    }
}
