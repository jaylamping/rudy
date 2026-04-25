use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::can::angle::PrincipalAngle;
use crate::can::motor_frame;
use crate::can::worker::MitStreamSetpoint;
use crate::inventory::{self, Actuator, Device};
use crate::state::SharedState;

use super::LinuxCanCore;

impl LinuxCanCore {
    /// Velocity-mode setpoint. The worker thread implements smart
    /// re-arm: on the first frame after `state.enabled` does NOT
    /// contain the role (or after a PP/MIT hold), the worker sends
    /// `cmd_stop` → `RUN_MODE = 2` → `SPD_REF` → `cmd_enable` — matching
    /// the bench bring-up order so `spd_ref` is latched before enable.
    /// On every subsequent frame (`state.enabled` already contains the
    /// role), it writes only `SPD_REF`. Cuts steady-state jog traffic
    /// from 60 to 20 frames/s.
    ///
    /// Velocity is *clamped* to the firmware-level `limit_spd`
    /// envelope before forwarding so a misbehaving caller can't bypass
    /// the firmware guard via the REST layer.
    pub fn set_velocity_setpoint(&self, motor: &Actuator, vel_rad_s: f32) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        // `vel_rad_s` is in cortex's logical frame (positive vel grows
        // positive position). Negate before write for motors whose
        // mechanical encoder reads opposite to the firmware command
        // sign — see `ActuatorCommon::direction_sign` for the
        // convention. Sign is applied symmetrically in
        // `worker/feedback.rs::apply_type2` and `handle/poll.rs` on
        // ingest, so the rest of cortex never sees the
        // firmware-native frame.
        let firmware_vel =
            motor_frame::firmware_scalar_from_logical(vel_rad_s, motor.common.direction_sign_f32());
        handle.set_velocity(
            self.host_id,
            motor.common.can_id,
            &motor.common.role,
            firmware_vel,
        )?;
        Ok(())
    }

    /// RS03 profile-position hold (`RUN_MODE=1`, `LOC_REF`, enable). `target` is principal-angle.
    ///
    /// `target` is in cortex's logical frame; the firmware-frame
    /// position written into `LOC_REF` is `target.raw() *
    /// direction_sign_f32()`. Negating a principal-angle in `(-π, π]`
    /// produces another value in `(-π, π]` (and `-π → -π` rather than
    /// `π`, but for hold targets within the operator-typical
    /// travel-limit envelope this is well-behaved).
    pub fn set_position_hold(&self, motor: &Actuator, target: PrincipalAngle) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        let firmware_target = motor_frame::firmware_scalar_from_logical(
            target.raw(),
            motor.common.direction_sign_f32(),
        );
        handle.set_position_hold(
            self.host_id,
            motor.common.can_id,
            &motor.common.role,
            firmware_target,
        )?;
        Ok(())
    }

    /// RS03 MIT spring-damper hold (`RUN_MODE=0` + single OperationCtrl frame).
    /// Used by [`crate::can::home_ramp::finish_home_success`]; resists droop
    /// and snaps back to `target` based on `kp`/`kd`, with no continuous
    /// servo command from the daemon.
    ///
    /// `target` is in the logical frame; sign-translated to the
    /// firmware frame the same way as [`Self::set_position_hold`] /
    /// [`Self::set_velocity_setpoint`]. `kp`/`kd` are torque-per-rad
    /// and torque-per-rad/s respectively — both invariant under
    /// position sign because torque on a sign-flipped motor flips
    /// direction in lockstep with the position error fed into the
    /// MIT control law: `tau = kp * (target_fw - pos_fw) + kd * (0 -
    /// vel_fw)`. Sign-flipping `target` and `pos` (and `vel`) in the
    /// firmware-side equation yields the same physical torque
    /// vector, so kp/kd pass through untouched.
    pub fn set_mit_hold(
        &self,
        motor: &Actuator,
        target: PrincipalAngle,
        kp_nm_per_rad: f32,
        kd_nm_s_per_rad: f32,
    ) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        let firmware_target = motor_frame::firmware_scalar_from_logical(
            target.raw(),
            motor.common.direction_sign_f32(),
        );
        handle.set_mit_hold(
            self.host_id,
            motor.common.can_id,
            &motor.common.role,
            firmware_target,
            kp_nm_per_rad,
            kd_nm_s_per_rad,
        )?;
        Ok(())
    }

    /// Streaming MIT `OperationCtrl` each control tick. Logical-frame
    /// `position_rad` / `velocity_rad_s` / `torque_ff_nm`; sign applied at
    /// the CAN boundary like [`Self::set_velocity_setpoint`].
    pub fn set_mit_command_stream(
        &self,
        motor: &Actuator,
        position_rad: f32,
        velocity_rad_s: f32,
        torque_ff_nm: f32,
        kp_nm_per_rad: f32,
        kd_nm_s_per_rad: f32,
    ) -> Result<()> {
        let handle = self.handle_for(&motor.common.can_bus)?;
        let sign = motor.common.direction_sign_f32();
        let firmware_pos = motor_frame::firmware_scalar_from_logical(position_rad, sign);
        let firmware_vel = motor_frame::firmware_scalar_from_logical(velocity_rad_s, sign);
        let firmware_torque_ff = motor_frame::firmware_scalar_from_logical(torque_ff_nm, sign);
        handle.set_mit_command(
            self.host_id,
            motor.common.can_id,
            &motor.common.role,
            MitStreamSetpoint {
                position_rad: firmware_pos,
                velocity_rad_s: firmware_vel,
                torque_ff_nm: firmware_torque_ff,
                kp_nm_per_rad,
                kd_nm_s_per_rad,
            },
        )?;
        Ok(())
    }

    /// Apply low torque/speed limits at boot from `inventory.desired_params`, or seed
    /// commissioning defaults into inventory on first run (write + save to flash).
    pub fn seed_boot_low_limits(&self, state: &SharedState) {
        let inv_path = state.cfg.paths.inventory.clone();
        let db_ctx = state.runtime_inventory_persist();
        let motors: Vec<Actuator> = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .actuators()
            .filter(|m| m.common.present)
            .cloned()
            .collect();

        for motor in motors {
            let spec = state.spec_for(motor.robstride_model());
            if !motor.common.desired_params.is_empty() {
                for (name, val) in &motor.common.desired_params {
                    if let Some(desc) = spec.firmware_limits.get(name) {
                        if let Err(e) = self.write_param(&motor, desc, val, true) {
                            tracing::warn!(
                                role = %motor.common.role,
                                param = %name,
                                error = ?e,
                                "boot-time desired_params write failed",
                            );
                        }
                    }
                }
                continue;
            }

            let limit_torque_nm = spec
                .commissioning_defaults
                .get("limit_torque_nm")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32);
            let limit_spd_rad_s = spec
                .commissioning_defaults
                .get("limit_spd_rad_s")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32);

            let mut fresh_desired: BTreeMap<String, serde_json::Value> = BTreeMap::new();

            if let (Some(t), Some(_)) = (limit_torque_nm, &motor.common.travel_limits) {
                if let Some(desc) = spec.firmware_limits.get("limit_torque") {
                    if self
                        .write_param(&motor, desc, &serde_json::json!(t), true)
                        .is_ok()
                    {
                        fresh_desired.insert("limit_torque".into(), serde_json::json!(t as f64));
                    } else {
                        tracing::warn!(
                            role = %motor.common.role,
                            "boot-time limit_torque write failed",
                        );
                    }
                }
            }
            if let Some(s) = limit_spd_rad_s {
                if let Some(desc) = spec.firmware_limits.get("limit_spd") {
                    if self
                        .write_param(&motor, desc, &serde_json::json!(s), true)
                        .is_ok()
                    {
                        fresh_desired.insert("limit_spd".into(), serde_json::json!(s as f64));
                    } else {
                        tracing::warn!(
                            role = %motor.common.role,
                            "boot-time limit_spd write failed",
                        );
                    }
                }
            }

            if fresh_desired.is_empty() {
                continue;
            }

            let role = motor.common.role.clone();
            match inventory::write_atomic(&inv_path, db_ctx.clone(), |inv| {
                let actuator = inv
                    .devices
                    .iter_mut()
                    .find_map(|device| match device {
                        Device::Actuator(a) if a.common.role == role => Some(a),
                        _ => None,
                    })
                    .ok_or_else(|| anyhow!("actuator {role} missing during seed"))?;
                for (k, v) in &fresh_desired {
                    actuator.common.desired_params.insert(k.clone(), v.clone());
                }
                Ok(())
            }) {
                Ok(new_inv) => {
                    *state.inventory.write().expect("inventory poisoned") = new_inv;
                    tracing::info!(role = %role, "boot-time persisted commissioning defaults to desired_params");
                }
                Err(e) => {
                    tracing::warn!(role = %role, error = ?e, "boot-time desired_params inventory write failed");
                }
            }
        }
    }

    /// Ensure every present RS03 motor has active type-2 reporting enabled
    /// at 100 Hz, and persist the setting once per motor.
    ///
    /// Per motor:
    /// 1) Always re-apply RAM-side `EPScan_time=1` + type-24 enable.
    /// 2) If inventory says it has not been persisted yet, issue type-22
    ///    save-to-flash, then flip `active_report_persisted=true` via
    ///    inventory::write_atomic.
    pub fn ensure_active_reporting_for_all(&self, state: &SharedState) {
        let motors: Vec<Actuator> = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .actuators()
            .filter(|m| m.common.present)
            .cloned()
            .collect();

        let inv_path = state.cfg.paths.inventory.clone();
        for motor in motors {
            let role = motor.common.role.clone();
            if let Err(e) = self.ensure_active_report_100hz(&motor) {
                tracing::warn!(role = %role, error = ?e, "boot active-report enable failed");
                continue;
            }

            if motor.common.active_report_persisted {
                continue;
            }

            if let Err(e) = self.save_to_flash(&motor) {
                tracing::warn!(role = %role, error = ?e, "boot active-report save-to-flash failed");
                continue;
            }

            // RS03 flash commit is asynchronous; match the existing
            // post-save settle window used by commission flow.
            std::thread::sleep(Duration::from_millis(100));

            match mark_active_report_persisted(state, &inv_path, &role) {
                Ok(true) => {
                    tracing::info!(role = %role, "boot active-report persisted to flash");
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(role = %role, error = ?e, "boot active-report inventory update failed");
                }
            }
        }
    }
}

fn mark_active_report_persisted(
    state: &SharedState,
    inv_path: &std::path::Path,
    role: &str,
) -> Result<bool> {
    {
        let inv = state.inventory.read().expect("inventory poisoned");
        if let Some(actuator) = inv.actuator_by_role(role) {
            if actuator.common.active_report_persisted {
                return Ok(false);
            }
        }
    }

    let role_owned = role.to_string();
    let new_inv = inventory::write_atomic(inv_path, state.runtime_inventory_persist(), |inv| {
        let actuator = inv
            .devices
            .iter_mut()
            .find_map(|device| match device {
                Device::Actuator(a) if a.common.role == role_owned => Some(a),
                _ => None,
            })
            .ok_or_else(|| anyhow!("role {role_owned} disappeared during inventory update"))?;
        actuator.common.active_report_persisted = true;
        Ok(())
    })?;
    *state.inventory.write().expect("inventory poisoned") = new_inv;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::audit::AuditLog;
    use crate::config::{
        CanConfig, Config, HttpConfig, LogsConfig, MotionBackend, PathsConfig, SafetyConfig,
        TelemetryConfig, WebTransportConfig,
    };
    use crate::inventory::Inventory;
    use crate::reminders::ReminderStore;
    use crate::spec;
    use crate::state::AppState;

    use super::mark_active_report_persisted;

    fn state_with_inventory_flag(
        active_report_persisted: bool,
    ) -> (
        crate::state::SharedState,
        tempfile::TempDir,
        std::path::PathBuf,
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec_path = dir.path().join("robstride_rs03.yaml");
        std::fs::write(
            &spec_path,
            "schema_version: 2\nactuator_model: RS03\nfirmware_limits: {}\nobservables: {}\n",
        )
        .expect("write spec");

        let inv_path = dir.path().join("inventory.yaml");
        std::fs::write(
            &inv_path,
            format!(
                "schema_version: 2\ndevices:\n  - kind: actuator\n    role: m\n    can_bus: can0\n    can_id: 1\n    present: true\n    verified: false\n    travel_limits:\n      min_rad: -1.0\n      max_rad: 1.0\n    commissioned_zero_offset: null\n    active_report_persisted: {active_report_persisted}\n    family:\n      kind: robstride\n      model: rs03\n"
            ),
        )
        .expect("write inventory");

        let cfg = Config {
            http: HttpConfig {
                bind: "127.0.0.1:0".into(),
            },
            webtransport: WebTransportConfig {
                bind: "127.0.0.1:0".into(),
                enabled: false,
                cert_path: None,
                key_path: None,
            },
            paths: PathsConfig {
                actuator_spec: spec_path.clone(),
                inventory: inv_path.clone(),
                inventory_seed: None,
                audit_log: dir.path().join("audit.jsonl"),
            },
            can: CanConfig {
                mock: true,
                buses: vec![],
            },
            telemetry: TelemetryConfig {
                poll_interval_ms: 10,
            },
            safety: SafetyConfig {
                require_verified: false,
                boot_max_step_rad: 0.087,
                step_size_rad: 0.004,
                tick_interval_ms: 10,
                homing_speed_rad_s: None,
                tracking_error_max_rad: 0.05,
                tracking_error_grace_ticks: 15,
                tracking_freshness_max_age_ms: 100,
                tracking_error_debounce_ticks: 15,
                band_violation_debounce_ticks: 15,
                boot_tracking_error_max_rad: 0.2,
                target_tolerance_rad: 0.005,
                target_dwell_ticks: 5,
                // Not under test here; disable the velocity gate.
                target_dwell_max_vel_rad_s: f32::INFINITY,
                homer_timeout_ms: 30_000,
                max_feedback_age_ms: 250,
                commission_readback_tolerance_rad: 1e-3,
                auto_home_on_boot: true,
                scan_on_boot: true,
                hold_kp_nm_per_rad: 10.0,
                hold_kd_nm_s_per_rad: 0.5,
                motion_backend: MotionBackend::Velocity,
                mit_command_rate_hz: 100.0,
                mit_max_angle_step_rad: 0.087,
                mit_lpf_cutoff_hz: 6.0,
                mit_min_jerk_blend_ms: 0.0,
            },
            logs: LogsConfig {
                db_path: dir.path().join("logs.db"),
                ..LogsConfig::default()
            },
            runtime: crate::config::RuntimeDbConfig::default(),
        };
        let specs = spec::load_robstride_specs(dir.path(), Some(&spec_path)).expect("load specs");
        let inv = Inventory::load(&inv_path).expect("load inventory");
        let audit = AuditLog::open(dir.path().join("audit.jsonl")).expect("open audit");
        let reminders = ReminderStore::open(dir.path().join("reminders.json")).expect("reminders");
        let state = Arc::new(AppState::new(cfg, specs, inv, audit, None, reminders));
        (state, dir, inv_path)
    }

    #[test]
    fn active_report_persist_flag_writes_once_then_is_idempotent() {
        let (state, _dir, inv_path) = state_with_inventory_flag(false);

        let first = mark_active_report_persisted(&state, &inv_path, "m").expect("first persist");
        assert!(first, "first call should persist");

        let on_disk = Inventory::load(&inv_path).expect("reload inventory");
        let motor = on_disk.actuator_by_role("m").expect("motor m exists");
        assert!(motor.common.active_report_persisted);

        let bytes_after_first = std::fs::read_to_string(&inv_path).expect("inventory text");
        let second = mark_active_report_persisted(&state, &inv_path, "m").expect("second persist");
        assert!(!second, "second call should be no-op");
        let bytes_after_second = std::fs::read_to_string(&inv_path).expect("inventory text");
        assert_eq!(bytes_after_first, bytes_after_second);
    }
}
