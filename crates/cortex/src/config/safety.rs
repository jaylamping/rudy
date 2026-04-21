//! Motion / homing safety limits and boot orchestrator tuning.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    #[serde(default = "default_true")]
    pub require_verified: bool,

    /// Per-step angular ceiling enforced on every command path while
    /// `BootState != Homed`. Default 5 deg ~= 0.087 rad. Catches large
    /// position commands that bypass the homer (or buggy clients).
    #[serde(default = "default_boot_max_step_rad")]
    pub boot_max_step_rad: f32,

    /// Per-tick step size for the slow-ramp homer. Default 0.02 rad ~= 1.1 deg.
    #[serde(default = "default_step_size_rad")]
    pub step_size_rad: f32,

    /// Tick interval for the slow-ramp loops, in milliseconds. Default 50
    /// ms; combined with `step_size_rad` gives ~22 deg/s effective speed.
    #[serde(default = "default_tick_interval_ms")]
    pub tick_interval_ms: u32,

    /// Maximum allowed `|setpoint - measured|` during a slow-ramp move.
    /// Exceeding this aborts the move (motor is bound up, or external
    /// force fighting it). Default 0.05 rad ~= 2.9 deg.
    #[serde(default = "default_tracking_error_max_rad")]
    pub tracking_error_max_rad: f32,

    /// Number of leading ticks during which the tracking-error abort is
    /// suppressed. The slow-ramp homer advances its setpoint by
    /// `step_size_rad` on every tick *before* sleeping, but the measured
    /// position lags by the firmware velocity-loop response time plus the
    /// type-2 telemetry pipeline (30-100 ms total on a cold motor that
    /// has just been re-armed by the bus_worker's RUN_MODE + cmd_enable
    /// sequence). Without this grace window, the homer aborts on tick 2
    /// or 3 every time it's asked to move a motor that hasn't been
    /// jogged yet this power-cycle — which is exactly the scenario the
    /// boot orchestrator runs in. Default 3 ticks (150 ms with default
    /// `tick_interval_ms`); the `homer_timeout_ms` ceiling still
    /// backstops a motor that genuinely refuses to move.
    #[serde(default = "default_tracking_error_grace_ticks")]
    pub tracking_error_grace_ticks: u32,

    /// Maximum age of `state.latest[role].t_ms` for a slow-ramp tick to treat
    /// telemetry as fresh. When stale or missing, the homer **holds**
    /// `setpoint_unwrapped` for that tick (no phantom tracking error from a
    /// frozen `mech_pos_rad` while the setpoint kept marching) and skips the
    /// tracking-error debounce for that tick. Default **100 ms** (~2× the
    /// default `tick_interval_ms`). Independent of `max_feedback_age_ms`,
    /// which gates jog and the boot orchestrator's *pre-flight* check only.
    /// `homer_timeout_ms` still backstops a motor that never reports fresh
    /// feedback again.
    #[serde(default = "default_tracking_freshness_max_age_ms")]
    pub tracking_freshness_max_age_ms: u64,

    /// Number of **consecutive** fresh ticks (after `tracking_error_grace_ticks`)
    /// with `|setpoint − measured| >` the active tracking budget required to
    /// abort with `tracking_error`. A single transient spike or one stale gap
    /// in telemetry no longer kills the whole home. Default **3**. Set to
    /// **1** to restore the legacy single-sample abort. `homer_timeout_ms`
    /// remains the ceiling for a motor that never converges.
    #[serde(default = "default_tracking_error_debounce_ticks")]
    pub tracking_error_debounce_ticks: u32,

    /// Boot-orchestrator-specific override for `tracking_error_max_rad`.
    /// The orchestrator runs unattended on cold motors at boot, so a
    /// looser budget than the operator-driven `POST /home` is
    /// appropriate: the operator path generally fires after the motor
    /// has been jogged in this session and the firmware loop is warm,
    /// while the boot path always starts from a dead stop AND has to
    /// drag gravity-loaded joints (shoulder_pitch, elbow_pitch, etc.)
    /// from arbitrary starting poses to the predefined home. The arm's
    /// moment about a pitch axis can demand several Nm of static torque
    /// the whole way, and the firmware velocity loop trades that off
    /// against the configured current limit by lagging the commanded
    /// velocity — which the homer sees as a growing tracking gap even
    /// though the motor is doing the best it can. Default 0.20 rad
    /// ~= 11.5 deg — four times the operator-path budget and roughly
    /// the worst-case sustained lag observed on a fully-extended arm
    /// homing through its mid-travel against gravity. A genuinely
    /// bound-up joint will still trip the 3-tick debounce in well
    /// under a second; the `homer_timeout_ms` ceiling backstops
    /// anything that converges arbitrarily slowly. Operator-driven
    /// homes (`POST /api/motors/:role/home`, `POST /api/home_all`)
    /// continue to use the tighter `tracking_error_max_rad` because
    /// the operator is at the keyboard and the motor is warm.
    #[serde(default = "default_boot_tracking_error_max_rad")]
    pub boot_tracking_error_max_rad: f32,

    /// Tolerance for "we have arrived at the target." Default 0.005 rad
    /// ~= 0.3 deg.
    #[serde(default = "default_target_tolerance_rad")]
    pub target_tolerance_rad: f32,

    /// Hard timeout on the slow-ramp loops, in milliseconds. Default 30 s.
    #[serde(default = "default_homer_timeout_ms")]
    pub homer_timeout_ms: u32,

    /// Maximum tolerated age of cached telemetry, in ms, on the jog path.
    /// If `state.latest[role]` is missing or older than this, the daemon
    /// refuses the jog with `409 stale_telemetry`. This is the fail-closed
    /// half of the "Sweep travel limits" safety hole: when bus contention
    /// or backoff freezes `state.latest`, the position-projection check
    /// would otherwise approve every subsequent jog forever.
    ///
    /// Default 250 ms. The original 100 ms target matched the type-2
    /// hot-path cadence (~16 ms at 60 Hz), but on a real bus with N
    /// idle motors the type-17 fallback round-robin sits at roughly
    /// `poll_interval_ms × N + slack` per role — easily 100-200 ms when
    /// the motor isn't actively emitting type-2 frames. 250 ms absorbs
    /// that worst-case fallback gap (still ~15 missed 60 Hz frames) so
    /// the very first jog out of idle isn't a guaranteed false positive,
    /// while staying tight enough that a true mid-sweep type-2 stall
    /// fails closed within ~4 SPA tick budgets. The SPA mirror in
    /// `motion-tests-card.tsx` uses the same threshold so the client
    /// stops sending before the server refuses.
    #[serde(default = "default_max_feedback_age_ms")]
    pub max_feedback_age_ms: u64,

    /// Tolerance for the boot orchestrator's add_offset readback check.
    /// On every boot the orchestrator reads `add_offset` (0x702B) over
    /// CAN and compares it against the `commissioned_zero_offset`
    /// recorded in `inventory.yaml`; a mismatch larger than this lands
    /// the motor in `BootState::OffsetChanged` and refuses motion until
    /// the operator either re-commissions or restores. Default 1e-3 rad
    /// (~0.057°): tight enough to catch a deliberate set_zero from the
    /// bench tool, loose enough to ignore the usual firmware-side
    /// rounding when the same float survives a flash round-trip.
    #[serde(default = "default_commission_readback_tolerance_rad")]
    pub commission_readback_tolerance_rad: f32,

    /// Master switch for the boot orchestrator's auto-home flow.
    /// With `true` (the operator-confirmed default), every commissioned
    /// motor whose first valid telemetry lands `InBand` is automatically
    /// driven to its `predefined_home_rad` via the slow-ramp homer; the
    /// operator never has to click "Verify & Home" on every boot.
    /// With `false` the orchestrator never spawns an auto-home —
    /// commissioned motors then need the manual `Verify & Home` flow,
    /// exactly like uncommissioned motors. Useful as an escape hatch
    /// during a hardware investigation; the operator can flip this off
    /// and restart the daemon without losing any commissioning state.
    #[serde(default = "default_true")]
    pub auto_home_on_boot: bool,

    /// Run `POST /api/hardware/scan` once after SocketCAN workers start.
    /// Disable on noisy benches or when startup latency matters.
    #[serde(default = "default_true")]
    pub scan_on_boot: bool,
}

fn default_true() -> bool {
    true
}

pub(crate) fn default_boot_max_step_rad() -> f32 {
    0.087
}

pub(crate) fn default_commission_readback_tolerance_rad() -> f32 {
    1e-3
}

pub(crate) fn default_step_size_rad() -> f32 {
    0.02
}

pub(crate) fn default_tick_interval_ms() -> u32 {
    50
}

pub(crate) fn default_tracking_error_max_rad() -> f32 {
    0.05
}

pub(crate) fn default_tracking_error_grace_ticks() -> u32 {
    3
}

pub(crate) fn default_tracking_freshness_max_age_ms() -> u64 {
    100
}

pub(crate) fn default_tracking_error_debounce_ticks() -> u32 {
    3
}

pub(crate) fn default_boot_tracking_error_max_rad() -> f32 {
    0.20
}

pub(crate) fn default_target_tolerance_rad() -> f32 {
    0.005
}

pub(crate) fn default_homer_timeout_ms() -> u32 {
    30_000
}

pub(crate) fn default_max_feedback_age_ms() -> u64 {
    250
}
