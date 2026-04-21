//! Bench routine / test harness wire types.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bench-routine name accepted by `POST /api/motors/:role/tests/:name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(rename_all = "snake_case")]
pub enum TestName {
    Read,
    SetZero,
    Smoke,
    Jog,
    JogOverlimit,
}

impl TestName {
    pub fn as_str(self) -> &'static str {
        match self {
            TestName::Read => "read",
            TestName::SetZero => "set_zero",
            TestName::Smoke => "smoke",
            TestName::Jog => "jog",
            TestName::JogOverlimit => "jog_overlimit",
        }
    }
}

/// Severity for one [`TestProgress`] line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(rename_all = "snake_case")]
pub enum TestLevel {
    Info,
    Warn,
    Pass,
    Fail,
}

/// One progress line for a running bench routine. Streamed reliably on the
/// `test_progress` WebTransport stream.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct TestProgress {
    pub run_id: String,
    pub role: String,
    /// Per-run monotonic line counter; the SPA uses it for the React key so
    /// every line lands exactly once even if the WT stream re-anchors.
    pub seq: u64,
    pub t_ms: i64,
    /// Coarse step name (e.g. `"sanity"`, `"ramp_up"`, `"defang"`). Helps
    /// the operator scan a long log.
    pub step: String,
    pub level: TestLevel,
    pub message: String,
}
