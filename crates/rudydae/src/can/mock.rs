//! Mock CAN core: synthesizes plausible feedback for each motor in the
//! inventory so the REST + WebTransport stack is fully exercisable without
//! hardware. Deterministic enough to be readable; noisy enough to be lively.

use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::time::interval;
use tracing::info;

use crate::boot_state::{self, ClassifyOutcome, BootState};
use crate::can::auto_recovery;
use crate::state::SharedState;
use crate::types::MotorFeedback;

pub fn spawn(state: SharedState) -> Result<()> {
    let period = Duration::from_millis(state.cfg.telemetry.poll_interval_ms.max(10));
    info!(
        period_ms = period.as_millis() as u64,
        "mock CAN core spawned"
    );

    tokio::spawn(async move {
        let mut tick = interval(period);
        let start = std::time::Instant::now();
        loop {
            tick.tick().await;
            let t = start.elapsed().as_secs_f32();
            let motors = state
                .inventory
                .read()
                .expect("inventory poisoned")
                .motors
                .clone();
            for (i, motor) in motors.iter().enumerate() {
                let phase = i as f32 * 0.9;
                let fb = MotorFeedback {
                    t_ms: Utc::now().timestamp_millis(),
                    role: motor.role.clone(),
                    can_id: motor.can_id,
                    mech_pos_rad: (t * 0.7 + phase).sin() * 0.8,
                    mech_vel_rad_s: (t * 0.7 + phase).cos() * 0.7 * 0.8,
                    torque_nm: (t * 1.3 + phase).sin() * 0.15,
                    vbus_v: 48.0 + (t * 0.05).sin() * 0.3,
                    temp_c: 35.0 + (t * 0.02).sin() * 1.5 + i as f32 * 2.0,
                    fault_sta: 0,
                    warn_sta: 0,
                };

                state
                    .latest
                    .write()
                    .expect("latest poisoned")
                    .insert(motor.role.clone(), fb.clone());

                // Run the boot-time classifier. On the first OutOfBand
                // transition for a present motor, maybe-spawn auto-recovery.
                if let ClassifyOutcome::Changed {
                    new: BootState::OutOfBand { mech_pos_rad, .. },
                    ..
                } = boot_state::classify(&state, &motor.role, fb.mech_pos_rad)
                {
                    auto_recovery::maybe_spawn_recovery(
                        &state,
                        &motor.role,
                        mech_pos_rad,
                    );
                }

                // Ignore errors: no WebTransport subscribers yet is fine.
                let _ = state.feedback_tx.send(fb);
            }
        }
    });

    Ok(())
}
