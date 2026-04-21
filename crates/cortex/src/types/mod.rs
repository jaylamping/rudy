//! Wire types shared between cortex and the `link` SPA.
//!
//! Every type here has `#[derive(TS)] #[ts(export, export_to = "...")]`, so
//! `cargo test -p cortex export_bindings` regenerates `link/src/lib/types/*.ts`.
//! `crates/.cargo/config.toml` sets `TS_RS_EXPORT_DIR` so outputs land next to the SPA.
//! Run `python scripts/fix-ts-rs-imports.py` (or `npm run gen:types` in `link/`) to fix serde_json paths. See
//! <https://github.com/Aleph-Alpha/ts-rs>.

mod logs;
mod meta;
mod motor;
mod reminders;
mod safety;
mod system;
mod tests;
mod wt;

pub use logs::{LogEntry, LogFilterDirective, LogFilterState, LogLevel, LogSource};
pub use meta::{ServerConfig, ServerFeatures, WebTransportAdvert};
pub use motor::{
    ApiError, LimbQuarantineMotor, MotorFeedback, MotorSummary, ParamSnapshot, ParamValue,
    ParamWrite,
};
pub use reminders::{Reminder, ReminderInput};
pub use safety::SafetyEvent;
pub use system::{SystemSnapshot, SystemTemps, SystemThrottled};
pub use tests::{TestLevel, TestName, TestProgress};
pub use wt::{
    WtEnvelope, WtFrame, WtKind, WtPayload, WtStreamMeta, WtSubscribe, WtSubscribeFilters,
    WtTransport, WT_PROTOCOL_VERSION, WT_STREAMS,
};
