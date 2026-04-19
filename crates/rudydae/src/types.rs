//! Wire types shared between rudydae and the `link` SPA.
//!
//! Every type here has `#[derive(TS)] #[ts(export, export_to = "...")]`, so
//! `cargo test -p rudydae export_bindings` regenerates `link/src/lib/types/*.ts`.
//! `crates/.cargo/config.toml` sets `TS_RS_EXPORT_DIR` so outputs land next to the SPA.
//! Run `python scripts/fix-ts-rs-imports.py` (or `npm run gen:types` in `link/`) to fix serde_json paths. See
//! <https://github.com/Aleph-Alpha/ts-rs>.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// GET /api/config — what the UI needs to bootstrap.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ServerConfig {
    pub version: String,
    pub actuator_model: String,
    pub webtransport: WebTransportAdvert,
    pub features: ServerFeatures,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct WebTransportAdvert {
    pub enabled: bool,
    /// Fully-qualified URL the browser should open. Example:
    /// `https://rudy.your-tailnet.ts.net:4433/wt`.
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ServerFeatures {
    pub mock_can: bool,
    pub require_verified: bool,
}

/// GET /api/motors — list summary.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct MotorSummary {
    pub role: String,
    pub can_bus: String,
    pub can_id: u8,
    pub firmware_version: Option<String>,
    pub verified: bool,
    pub present: bool,
    pub travel_limits: Option<crate::inventory::TravelLimits>,
    pub latest: Option<MotorFeedback>,
    /// Per-power-cycle gate state. UI renders a colored badge driven off
    /// the variant; OutOfBand carries enough detail to display
    /// "X.X deg outside [Y.Y, Z.Z]" without a second roundtrip.
    pub boot_state: crate::boot_state::BootState,
    /// Limb assignment, if any (`left_arm`, `right_leg`, etc.). None for
    /// ungrouped motors that haven't been assigned via the inventory tab.
    pub limb: Option<String>,
    /// Canonical position in the kinematic chain. None for ungrouped motors.
    pub joint_kind: Option<crate::limb::JointKind>,
}

/// One snapshot of telemetry for a motor. Sent:
/// - as JSON from `GET /api/motors/:role/feedback` (polled),
/// - as CBOR from WebTransport datagrams (pushed).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct MotorFeedback {
    /// Milliseconds since unix epoch, for trivial client-side ordering.
    pub t_ms: i64,
    pub role: String,
    pub can_id: u8,
    pub mech_pos_rad: f32,
    pub mech_vel_rad_s: f32,
    pub torque_nm: f32,
    pub vbus_v: f32,
    pub temp_c: f32,
    pub fault_sta: u32,
    pub warn_sta: u32,
}

/// GET /api/motors/:role/params — full catalog snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ParamSnapshot {
    pub role: String,
    pub values: BTreeMap<String, ParamValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ParamValue {
    pub name: String,
    pub index: u16,
    #[serde(rename = "type")]
    pub ty: String,
    pub units: Option<String>,
    pub value: serde_json::Value,
    pub hardware_range: Option<[f32; 2]>,
}

/// PUT /api/motors/:role/params/:index body.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ParamWrite {
    pub value: serde_json::Value,
    /// If `true`, rudydae also issues the type-22 save after the write. If
    /// `false` (default), the value lives in RAM and `POST /api/motors/:role/save`
    /// is required to persist it.
    #[serde(default)]
    pub save_after: bool,
}

/// Standard error envelope for API responses.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ApiError {
    pub error: String,
    pub detail: Option<String>,
}

/// GET /api/system - host metrics for the operator-console dashboard.
///
/// Linux real values come from `/proc` + `/sys` + (on the Pi) `vcgencmd`;
/// when `cfg.can.mock == true` or running on non-Linux, fields are
/// slowly-varying mock numbers and `is_mock = true`. See `system.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SystemSnapshot {
    /// Wallclock at sample time, ms since unix epoch.
    pub t_ms: i64,
    pub cpu_pct: f32,
    /// 1, 5, 15-minute load average from `/proc/loadavg`.
    pub load: [f32; 3],
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub temps_c: SystemTemps,
    pub throttled: SystemThrottled,
    pub uptime_s: u64,
    pub hostname: String,
    pub kernel: String,
    /// True when values are synthetic (no Linux host or `cfg.can.mock = true`).
    pub is_mock: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SystemTemps {
    pub cpu: Option<f32>,
    pub gpu: Option<f32>,
}

/// Pi-specific power/thermal throttling state. `now` and `ever` are derived
/// from `vcgencmd get_throttled` bits (0/2 -> now, 16/18 -> ever).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SystemThrottled {
    pub now: bool,
    pub ever: bool,
    pub raw_hex: Option<String>,
}

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

/// Reliable broadcast for safety-relevant transitions.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SafetyEvent {
    Estop {
        t_ms: i64,
        source: String,
    },
    LockChanged {
        t_ms: i64,
        holder: Option<String>,
    },
    TravelLimitViolation {
        t_ms: i64,
        role: String,
        attempted_rad: f32,
        min_rad: f32,
        max_rad: f32,
    },
    /// Layer 6 began driving a motor back toward its band.
    AutoRecoveryAttempted {
        t_ms: i64,
        role: String,
        from_rad: f32,
        target_rad: f32,
        delta_rad: f32,
    },
    /// Layer 6 reached the in-band target; motor was left disabled. The
    /// operator still needs to do the Verify & Home ritual to enable.
    AutoRecoverySucceeded {
        t_ms: i64,
        role: String,
        final_pos_rad: f32,
        ticks: u32,
    },
    /// Layer 6 aborted (resistance / fault / timeout / budget exceeded).
    /// Operator must manually move the joint into band.
    AutoRecoveryFailed {
        t_ms: i64,
        role: String,
        reason: String,
        last_pos_rad: f32,
    },
    /// Layer 6 refused to start (delta to band exceeds budget, or feature
    /// disabled). No motion frame was sent on the bus.
    AutoRecoveryRefused {
        t_ms: i64,
        role: String,
        reason: String,
        delta_rad: f32,
    },
    /// Slow-ramp homer reached its target. Full torque/speed limits restored.
    Homed {
        t_ms: i64,
        role: String,
        final_pos_rad: f32,
        samples_count: u32,
    },
    /// A motor's role was changed at runtime (operator clicked Rename or
    /// Assign). Subscribers should drop any per-role caches keyed by the
    /// old role.
    MotorRenamed {
        t_ms: i64,
        old_role: String,
        new_role: String,
    },
    /// `POST /api/motors/:role/commission` completed successfully:
    /// firmware accepted type-6 SetZero + type-22 SaveParams, and the
    /// daemon read back `add_offset` (0x702B) and recorded it in
    /// `inventory.yaml` as `commissioned_zero_offset`. The boot
    /// orchestrator will use this stored value on every subsequent boot
    /// for the Class-1 shenanigan check (re-read `add_offset` over CAN
    /// and compare against this baseline within
    /// `safety.commission_readback_tolerance_rad`).
    ///
    /// Subscribers (the dashboard) should refresh the actuator list so
    /// the UI flips the motor from "Not commissioned" → "Commissioned"
    /// without waiting for the next polling cycle.
    Commissioned {
        t_ms: i64,
        role: String,
        offset_rad: f32,
    },
    /// Boot orchestrator detected a Class-1 shenanigan: the firmware's
    /// reported `add_offset` (0x702B) disagrees with the
    /// `commissioned_zero_offset` recorded in `inventory.yaml` by more
    /// than `safety.commission_readback_tolerance_rad`. Motion is
    /// refused until the operator either re-commissions
    /// (`POST /commission`, the new position becomes the recorded
    /// zero) or restores (`POST /restore_offset`, the daemon writes
    /// the stored value back to firmware and re-saves to flash).
    OffsetChanged {
        t_ms: i64,
        role: String,
        stored_rad: f32,
        current_rad: f32,
    },
    /// Boot orchestrator's slow-ramp homer aborted (tracking error,
    /// fault, timeout, path violation). `BootState::HomeFailed` will
    /// stick until the operator hits `POST /api/motors/:role/home` to
    /// retry. Distinct from `AutoRecoveryFailed` (which Layer 6 emits
    /// — Phase H.1 deletes that) and from a manual-homer abort (which
    /// already audit-logs but does not get its own SafetyEvent today).
    HomeFailed {
        t_ms: i64,
        role: String,
        reason: String,
        last_pos_rad: f32,
    },
    /// Boot orchestrator's slow-ramp homer reached its target without
    /// operator intervention. Distinct from `Homed` (which the manual
    /// homer endpoint emits) so dashboards can tell apart "operator
    /// clicked Verify & Home" from "boot orchestrator drove this on
    /// its own".
    AutoHomed {
        t_ms: i64,
        role: String,
        from_rad: f32,
        target_rad: f32,
        ticks: u32,
    },
}

/// One operator reminder. File-backed in `.rudyd/reminders.json`.
/// Created/edited/deleted via `/api/reminders[/:id]`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct Reminder {
    pub id: String,
    pub text: String,
    /// Optional ISO 8601 due date; the UI renders relative ("in 2h", "overdue").
    pub due_at: Option<String>,
    pub done: bool,
    /// Wallclock at creation, ms since unix epoch.
    pub created_ms: i64,
}

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
/// the tracing subscriber (rudydae internals, axum traces, etc.); `Audit`
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
/// default level (`info` in `info,rudydae=debug`).
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

/// POST /api/reminders body and PUT /api/reminders/:id body.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ReminderInput {
    pub text: String,
    pub due_at: Option<String>,
    #[serde(default)]
    pub done: bool,
}

/// WebTransport subscription request (sent on a bidirectional stream by the
/// client right after session open).
///
/// Backwards compatibility: a session that *never* sends `WtSubscribe` gets
/// every stream the server knows about. This keeps the contract simple for
/// dumb clients (curl/wt-cli, future Python tooling) and lets the SPA evolve
/// its filter without coordinating with the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct WtSubscribe {
    /// Stream kinds the client wants. Empty list ≡ "all" (matches the
    /// default-when-no-subscribe behavior).
    pub kinds: Vec<WtKind>,
    /// Optional per-kind narrow filters (e.g. motor roles). Today only
    /// `motor_feedback` honors the value; unknown keys are ignored.
    #[serde(default)]
    pub filters: WtSubscribeFilters,
}

/// Per-kind narrow filters. Each field is optional; `None`/empty means
/// "no narrowing" (i.e. all values for that kind).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct WtSubscribeFilters {
    /// Motor roles the client cares about for `motor_feedback` frames.
    /// Empty / missing ≡ all roles.
    #[serde(default)]
    pub motor_roles: Vec<String>,
    /// Run ids the client cares about for `test_progress` frames.
    /// Empty / missing ≡ every run on the bus.
    #[serde(default)]
    pub run_ids: Vec<String>,
}

/// Current WebTransport envelope schema version.
///
/// Bumped when the *envelope* shape changes (not when payload structs evolve;
/// those are governed by the codec test plus ts-rs codegen). Decoders must
/// reject envelopes whose `v` field doesn't match.
pub const WT_PROTOCOL_VERSION: u8 = 1;

/// Network reliability tier for a WebTransport stream.
///
/// Matches the two transport options QUIC offers:
/// - `Datagram`: unreliable, unordered, no head-of-line blocking. Ideal for
///   high-rate "latest wins" telemetry where a dropped sample is harmless.
/// - `Stream`: reliable, in-order, lossless. Used for events that must
///   arrive (faults, command acks, log lines).
///
/// The router (`wt::router`) chooses the QUIC mechanism per frame based on
/// this value: datagrams ride `connection.send_datagram(...)`; streams ride a
/// long-lived uni-stream per session, length-prefixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WtTransport {
    Datagram,
    Stream,
}

/// Trait every WT payload type implements. Generated by `declare_wt_streams!`
/// — don't hand-impl. Provides the kind discriminator + transport hint at
/// compile time so the router can dispatch generically.
pub trait WtPayload: Serialize + Send + Sync + 'static {
    /// snake_case discriminator emitted as the `kind` field of the envelope.
    const KIND: &'static str;
    /// Reliability tier this payload requires.
    const TRANSPORT: WtTransport;
}

/// Declarative registry for WebTransport streams.
///
/// One macro entry per stream; expands to:
///   1. `pub enum WtFrame` with one variant per stream (used only for
///      *typed* tests — the wire shape is constructed via `WtEnvelope`).
///   2. `impl WtPayload` for each payload type, encoding the kind + transport.
///   3. `pub enum WtKind { ... }` — the typed discriminator used by
///      `WtSubscribe` + the router's filter.
///   4. `pub static WT_STREAMS: &[WtStreamMeta]` for runtime introspection
///      (used by docs / debug endpoints + by `wt::router::default_filter`).
///
/// Wire format reminder: every datagram is a CBOR map of the shape
/// `{ v, kind, seq, t_ms, data }` (see `WtEnvelope`). The macro does NOT
/// emit the envelope itself — it stays a hand-written struct so its fields
/// are debuggable and ts-rs-exported in one place.
///
/// To add a stream:
/// 1. Define the payload struct (e.g. `pub struct Fault { ... }`) with
///    `#[derive(Serialize, Deserialize, Clone, TS)]`.
/// 2. Add a line to the `declare_wt_streams!` invocation below.
/// 3. Add a `broadcast::Sender<Payload>` field to `AppState` and a producer.
/// 4. (Optional, frontend) register a reducer in `WebTransportBridge`.
///
/// That's it — no edits to `wt.rs`, no edits to the TS decoder.
macro_rules! declare_wt_streams {
    ( $(
        $variant:ident => $payload:ty {
            kind: $kind:literal,
            transport: $transport:ident,
            $(#[$variant_attr:meta])*
        }
    ),+ $(,)? ) => {
        /// Compile-time list of every registered WT stream.
        #[derive(Debug, Clone, Copy)]
        pub struct WtStreamMeta {
            pub kind: &'static str,
            pub transport: WtTransport,
        }

        /// All streams declared via `declare_wt_streams!`. Order matches
        /// declaration; treat as stable for telemetry but not for wire
        /// (the `kind` string is the actual identity).
        pub static WT_STREAMS: &[WtStreamMeta] = &[
            $( WtStreamMeta { kind: $kind, transport: WtTransport::$transport } ),+
        ];

        $(
            impl WtPayload for $payload {
                const KIND: &'static str = $kind;
                const TRANSPORT: WtTransport = WtTransport::$transport;
            }
        )+

        /// Typed discriminator. Used by `WtSubscribe` to filter and by the
        /// router's per-stream sequence counter. The on-wire kind is the
        /// snake_case literal from the macro; serde renames keep it stable.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        #[ts(export, export_to = "./")]
        #[serde(rename_all = "snake_case")]
        pub enum WtKind {
            $(
                $(#[$variant_attr])*
                $variant
            ),+
        }

        impl WtKind {
            pub fn as_str(self) -> &'static str {
                match self {
                    $( WtKind::$variant => $kind ),+
                }
            }

            pub fn transport(self) -> WtTransport {
                match self {
                    $( WtKind::$variant => WtTransport::$transport ),+
                }
            }

            /// All variants — useful as the default filter when a session
            /// doesn't send `WtSubscribe`.
            pub fn all() -> &'static [WtKind] {
                &[ $( WtKind::$variant ),+ ]
            }
        }
    };
}

declare_wt_streams! {
    MotorFeedback => MotorFeedback {
        kind: "motor_feedback",
        transport: Datagram,
        /// One motor's latest telemetry sample. High-rate (~10 Hz × N motors).
    },
    SystemSnapshot => SystemSnapshot {
        kind: "system_snapshot",
        transport: Datagram,
        /// Host-system metrics (CPU / mem / temps / throttle) at 0.5 Hz.
    },
    TestProgress => TestProgress {
        kind: "test_progress",
        transport: Stream,
        /// One progress line for a running bench routine. Reliable so the
        /// pass/fail terminal line is never dropped.
    },
    SafetyEvent => SafetyEvent {
        kind: "safety_event",
        transport: Stream,
        /// E-stop / control-lock / travel-band events. Reliable.
    },
    MotionStatus => crate::motion::MotionStatus {
        kind: "motion_status",
        transport: Datagram,
        /// Live status from a server-side motion controller (sweep / wave /
        /// jog). Datagram tier because the controller refreshes at the bus's
        /// native cadence; one missed snapshot is harmless because the next
        /// one arrives ~16 ms later. The terminal `state = stopped` snapshot
        /// is broadcast once per run and is the operator's "stop confirmed"
        /// signal — if it's lost in transit the SPA falls back to the
        /// `GET /motors/:role/motion` 204 to confirm.
    },
    LogEvent => LogEntry {
        kind: "log_event",
        transport: Stream,
        /// One captured log entry (tracing or audit), live-tailed to the
        /// Logs page. Reliable so the operator never misses an error
        /// during an incident; the SPA caps its in-memory buffer to 5k
        /// most-recent entries to bound memory regardless of session
        /// length.
    },
}

/// Wire envelope for every WebTransport frame, datagram or stream.
///
/// Generic over the payload `T` so the encoder can stay strongly-typed. Every
/// `T` must implement `WtPayload` (which the `declare_wt_streams!` macro emits
/// for you), giving the encoder access to `T::KIND` so the wire `kind` field
/// is impossible to typo.
///
/// Layout pinned by `tests/wt_codec.rs`:
/// ```cbor
/// {
///   "v": 1,                    # protocol version (WT_PROTOCOL_VERSION)
///   "kind": "motor_feedback",  # snake_case stream discriminator
///   "seq": 12345,              # per-stream sequence; client detects gaps
///   "t_ms": 1700000123456,     # wallclock at emit (envelope-level, not
///                              #   payload-level — different from any
///                              #   `t_ms` the payload may also carry)
///   "data": { ...payload... }  # nested; opaque to the envelope
/// }
/// ```
///
/// `data` is intentionally *nested* (not flattened via `#[serde(flatten)]`)
/// so payload field names can never collide with envelope field names. A
/// future payload that happens to define a `kind` or `seq` field doesn't
/// silently corrupt the envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WtEnvelope<T> {
    /// Envelope schema version. See `WT_PROTOCOL_VERSION`.
    pub v: u8,
    /// Stream kind (snake_case). Matches `T::KIND`.
    pub kind: &'static str,
    /// Per-stream monotonically-increasing sequence. Wraps at `u64::MAX`
    /// (~5×10^11 years at 1 kHz; not a practical concern). Lets clients
    /// detect dropped datagrams without parsing the payload.
    pub seq: u64,
    /// Wallclock at envelope emission, ms since unix epoch.
    pub t_ms: i64,
    pub data: T,
}

impl<T: WtPayload> WtEnvelope<T> {
    /// Build a fresh envelope with `KIND` filled in from the payload type
    /// and `t_ms` from the system clock. The router fills `seq`.
    pub fn new(seq: u64, data: T) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self {
            v: WT_PROTOCOL_VERSION,
            kind: T::KIND,
            seq,
            t_ms,
            data,
        }
    }
}

/// Tagged-union view of every possible payload, used for tests and for the
/// generated TypeScript discriminated union. The on-wire encoding is
/// `WtEnvelope<T>` for some specific `T`; this enum is what the *frontend*
/// sees after decoding (`{kind, data}` with `data` typed by `kind`).
///
/// The variants are kept in lockstep with `declare_wt_streams!` by hand for
/// now — small enough to be a non-issue, and ts-rs needs the explicit listing
/// to generate the discriminated union for `link/src/lib/types/WtFrame.ts`.
/// If we ever have >5 streams we can revisit with a doc-generation step.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum WtFrame {
    MotorFeedback(MotorFeedback),
    SystemSnapshot(SystemSnapshot),
    TestProgress(TestProgress),
    SafetyEvent(SafetyEvent),
    MotionStatus(crate::motion::MotionStatus),
    LogEvent(LogEntry),
}
