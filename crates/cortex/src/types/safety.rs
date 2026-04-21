//! Safety-event wire types (WebTransport + dashboard).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
    /// A motor was removed from inventory at runtime. Subscribers should
    /// purge any per-role state keyed by this role and refresh the hardware
    /// list (it may now appear under "Unassigned" if still present on CAN).
    MotorRemoved {
        t_ms: i64,
        role: String,
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
    /// Boot orchestrator's home-ramp homer aborted (tracking error,
    /// fault, timeout, path violation). `BootState::HomeFailed` will
    /// stick until the operator hits `POST /api/motors/:role/home` to
    /// retry. Distinct from a manual-homer abort (which audit-logs but
    /// does not emit `HomeFailed` on the safety stream today).
    HomeFailed {
        t_ms: i64,
        role: String,
        reason: String,
        last_pos_rad: f32,
    },
    /// Boot orchestrator's home-ramp homer reached its target without
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
