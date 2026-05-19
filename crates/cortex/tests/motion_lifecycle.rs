//! End-to-end exercise of the daemon-side motion lifecycle.
//!
//! Boots an `AppState` with a verified motor, seeds the
//! preconditions the controller's per-tick preflight expects (homed
//! boot state, fresh telemetry, travel limits), starts a sweep through
//! the registry, observes a `Running` `MotionStatus` on the broadcast
//! channel, stops the motion, and asserts:
//!
//!   * the terminal `Stopped` frame is emitted,
//!   * its `reason` is `"operator"`,
//!   * `state.enabled` is cleared for the role (the controller's exit
//!     gate ran),
//!   * the registry's `current()` returns `None` for the role.
//!
//! This is the canary for "did anyone break the controller's exit
//! discipline" — the same property that prevented the original jitter
//! issue from auto-recovering after a stop.

use std::time::Duration;

use cortex::config::MotionBackend;
use cortex::inventory::TravelLimits;
use cortex::motion::{MotionIntent, MotionState};
use cortex::state::SharedState;

mod common;

/// In-process travel-limits seeder. The production write path goes
/// through `inventory::write_atomic`, but for these tests we just want
/// the in-memory motor record to carry limits — the controller reads
/// them through `inventory.read().by_role(...)`.
fn set_travel_limits(state: &SharedState, role: &str, min_rad: f32, max_rad: f32) {
    let mut inv = state.inventory.write().expect("inventory poisoned");
    let a = common::actuator_mut(&mut inv, role)
        .unwrap_or_else(|| panic!("inventory missing role {role}"));
    a.common.travel_limits = Some(TravelLimits {
        min_rad,
        max_rad,
        updated_at: None,
    });
}

async fn wait_for_state(
    rx: &mut tokio::sync::broadcast::Receiver<cortex::motion::MotionStatus>,
    run_id: &str,
    state: MotionState,
    timeout: Duration,
) -> cortex::motion::MotionStatus {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("timed out waiting for {state:?} frame for run {run_id}");
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(frame)) if frame.run_id == run_id && frame.state == state => return frame,
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => panic!("motion_status_tx closed unexpectedly: {e}"),
            Err(_) => panic!("timed out waiting for {state:?} frame for run {run_id}"),
        }
    }
}

#[tokio::test]
async fn sweep_lifecycle_running_then_operator_stop() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);

    let role = "right_arm.shoulder_roll";

    // The sweep pattern requires travel limits; mutate the in-memory
    // inventory directly (write_atomic round-trips to disk and isn't
    // necessary for an in-process test).
    set_travel_limits(&state, role, -0.5, 0.5);

    // Subscribe BEFORE starting so we don't miss the initial Running
    // frame the controller emits before its first tick.
    let mut status_rx = state.motion_status_tx.subscribe();

    let run_id = state
        .motion
        .start(
            &state,
            role,
            MotionIntent::Sweep {
                speed_rad_s: 0.1,
                turnaround_rad: 0.05,
            },
        )
        .await
        .expect("start");

    // Drain frames until we see at least one Running status for *our*
    // run_id with a non-trivial position. Cap the wait so a stalled
    // controller fails fast rather than hanging the test runner.
    let mut saw_running = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while !saw_running {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("controller never emitted a Running status frame");
        }
        let frame = match tokio::time::timeout(remaining, status_rx.recv()).await {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => panic!("motion_status_tx closed unexpectedly: {e}"),
            Err(_) => panic!("timed out waiting for first Running frame"),
        };
        if frame.run_id != run_id {
            continue;
        }
        if frame.state == MotionState::Running {
            assert_eq!(frame.role, role);
            assert_eq!(frame.kind, "sweep");
            saw_running = true;
        }
    }

    // The registry should report this run as current.
    let snap = state.motion.current(role).expect("current");
    assert_eq!(snap.run_id, run_id);
    assert_eq!(snap.kind, "sweep");

    // Operator-driven stop.
    let was_running = state.motion.stop(role).await;
    assert!(was_running, "stop() should report a motion was running");

    // Wait for the terminal Stopped frame for our run_id.
    let mut saw_stopped = false;
    let mut last_reason: Option<String> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while !saw_stopped {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("controller never emitted a terminal Stopped frame");
        }
        let frame = match tokio::time::timeout(remaining, status_rx.recv()).await {
            Ok(Ok(f)) => f,
            Ok(Err(_)) => break,
            Err(_) => panic!("timed out waiting for Stopped frame"),
        };
        if frame.run_id != run_id {
            continue;
        }
        if frame.state == MotionState::Stopped {
            saw_stopped = true;
            last_reason = frame.reason.clone();
        }
    }
    assert!(saw_stopped, "no Stopped frame observed");
    assert_eq!(
        last_reason.as_deref(),
        Some("operator_hold"),
        "canonical shoulder_roll should hold on graceful operator stop"
    );

    // Registry slot is cleared.
    assert!(
        state.motion.current(role).is_none(),
        "registry should have no active motion for {role} after stop"
    );

    // Controller's exit gate cleared the per-motor enabled flag (mock
    // CAN, so the actual cmd_stop is a no-op, but the bookkeeping
    // still matters — the bus_worker re-arm logic depends on it).
    let enabled = state
        .enabled
        .read()
        .expect("enabled poisoned")
        .contains(role);
    assert!(
        !enabled,
        "controller exit should have cleared state.enabled[{role}]"
    );
}

#[tokio::test]
async fn second_start_supersedes_the_first() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);

    let role = "right_arm.shoulder_roll";
    set_travel_limits(&state, role, -0.5, 0.5);

    let mut status_rx = state.motion_status_tx.subscribe();

    let first = state
        .motion
        .start(
            &state,
            role,
            MotionIntent::Sweep {
                speed_rad_s: 0.1,
                turnaround_rad: 0.05,
            },
        )
        .await
        .expect("first start");

    // Wait for at least one frame from the first run so we know it's
    // really running before we supersede.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("first run never produced a status frame");
        }
        match tokio::time::timeout(remaining, status_rx.recv()).await {
            Ok(Ok(f)) if f.run_id == first => break,
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => panic!("status channel closed: {e}"),
            Err(_) => panic!("timed out waiting for first run frame"),
        }
    }

    let second = state
        .motion
        .start(
            &state,
            role,
            MotionIntent::Wave {
                center_rad: 0.0,
                amplitude_rad: 0.2,
                speed_rad_s: 0.1,
                turnaround_rad: 0.02,
            },
        )
        .await
        .expect("second start");

    assert_ne!(first, second, "supersede must allocate a fresh run_id");

    // The first run's terminal frame should arrive with reason "superseded".
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut superseded = false;
    while !superseded {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("first run was never marked superseded");
        }
        match tokio::time::timeout(remaining, status_rx.recv()).await {
            Ok(Ok(f)) if f.run_id == first && f.state == MotionState::Stopped => {
                assert_eq!(f.reason.as_deref(), Some("superseded_hold"));
                superseded = true;
            }
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => panic!("status channel closed: {e}"),
            Err(_) => panic!("timed out waiting for supersede frame"),
        }
    }

    // Stop the survivor cleanly so the test doesn't leak the controller.
    state.motion.stop(role).await;
}

#[tokio::test]
async fn running_sweep_stops_on_stale_telemetry() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);

    let role = "right_arm.shoulder_roll";
    set_travel_limits(&state, role, -0.5, 0.5);
    let mut status_rx = state.motion_status_tx.subscribe();

    let run_id = state
        .motion
        .start(
            &state,
            role,
            MotionIntent::Sweep {
                speed_rad_s: 0.1,
                turnaround_rad: 0.05,
            },
        )
        .await
        .expect("start");

    let _ = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Running,
        Duration::from_secs(1),
    )
    .await;
    {
        let mut latest = state.latest.write().expect("latest poisoned");
        let fb = latest.get_mut(role).expect("seeded feedback");
        fb.t_ms = chrono::Utc::now().timestamp_millis() - 10_000;
    }

    let stopped = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Stopped,
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(stopped.reason.as_deref(), Some("stale_telemetry"));
    assert!(state.motion.current(role).is_none());
}

#[tokio::test]
async fn jog_stops_when_heartbeat_lapses() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);

    let role = "right_arm.shoulder_roll";
    set_travel_limits(&state, role, -0.5, 0.5);
    let clock = common::spawn_latest_timestamp_refresh(state.clone(), role, 0.0);
    let mut status_rx = state.motion_status_tx.subscribe();

    let run_id = state
        .motion
        .start(&state, role, MotionIntent::Jog { vel_rad_s: 0.1 })
        .await
        .expect("start");

    let stopped = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Stopped,
        Duration::from_secs(2),
    )
    .await;
    clock.abort();
    assert_eq!(stopped.reason.as_deref(), Some("heartbeat_lapsed"));
    assert!(state.motion.current(role).is_none());
}

#[tokio::test]
async fn mit_backend_sweep_lifecycle_runs_and_stops() {
    let (state, _dir) = common::make_state();
    {
        let mut effective = state.effective.write().expect("effective poisoned");
        effective.safety.motion_backend = MotionBackend::Mit;
    }
    common::force_homed(&state);
    common::seed_feedback(&state);

    let role = "right_arm.shoulder_roll";
    set_travel_limits(&state, role, -0.5, 0.5);
    let mut status_rx = state.motion_status_tx.subscribe();

    let run_id = state
        .motion
        .start(
            &state,
            role,
            MotionIntent::Sweep {
                speed_rad_s: 0.1,
                turnaround_rad: 0.05,
            },
        )
        .await
        .expect("start");

    let running = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Running,
        Duration::from_secs(1),
    )
    .await;
    assert_eq!(running.kind, "sweep");
    assert!(
        running.vel_rad_s.abs()
            <= state.read_effective().safety.mit_max_angle_step_rad
                / (state.read_effective().safety.tick_interval_ms as f32 / 1000.0)
                + 1e-5
    );

    assert!(state.motion.stop(role).await);
    let stopped = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Stopped,
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(stopped.reason.as_deref(), Some("operator_hold"));
}

// ---------------------------------------------------------------------------
// Stop-policy integration: hold-on-stop for gravity-loaded joints
// ---------------------------------------------------------------------------

/// Set `stop_behavior` on an in-memory inventory actuator.
fn set_stop_behavior(state: &SharedState, role: &str, behavior: cortex::motion::StopBehavior) {
    let mut inv = state.inventory.write().expect("inventory poisoned");
    let a = common::actuator_mut(&mut inv, role)
        .unwrap_or_else(|| panic!("inventory missing role {role}"));
    a.common.stop_behavior = behavior;
}

#[tokio::test]
async fn operator_stop_with_hold_behavior_emits_hold_reason() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);

    let role = "right_arm.shoulder_roll";
    set_travel_limits(&state, role, -0.5, 0.5);
    set_stop_behavior(&state, role, cortex::motion::StopBehavior::Hold);

    let mut status_rx = state.motion_status_tx.subscribe();

    let run_id = state
        .motion
        .start(
            &state,
            role,
            MotionIntent::Sweep {
                speed_rad_s: 0.1,
                turnaround_rad: 0.05,
            },
        )
        .await
        .expect("start");

    let _ = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Running,
        Duration::from_secs(1),
    )
    .await;

    assert!(state.motion.stop(role).await);

    let stopped = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Stopped,
        Duration::from_secs(2),
    )
    .await;

    // With stop_behavior=hold and homed state, reason is "operator_hold"
    assert_eq!(
        stopped.reason.as_deref(),
        Some("operator_hold"),
        "hold-configured joint should report operator_hold on graceful stop"
    );

    // Registry and enabled state still cleared
    assert!(state.motion.current(role).is_none());
    let enabled = state
        .enabled
        .read()
        .expect("enabled poisoned")
        .contains(role);
    assert!(!enabled);
}

#[tokio::test]
async fn fault_stop_with_hold_behavior_still_hard_stops() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);

    let role = "right_arm.shoulder_roll";
    set_travel_limits(&state, role, -0.5, 0.5);
    set_stop_behavior(&state, role, cortex::motion::StopBehavior::Hold);

    let mut status_rx = state.motion_status_tx.subscribe();

    let run_id = state
        .motion
        .start(
            &state,
            role,
            MotionIntent::Sweep {
                speed_rad_s: 0.1,
                turnaround_rad: 0.05,
            },
        )
        .await
        .expect("start");

    let _ = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Running,
        Duration::from_secs(1),
    )
    .await;

    // Simulate stale telemetry (fault condition)
    {
        let mut latest = state.latest.write().expect("latest poisoned");
        let fb = latest.get_mut(role).expect("seeded feedback");
        fb.t_ms = chrono::Utc::now().timestamp_millis() - 10_000;
    }

    let stopped = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Stopped,
        Duration::from_secs(2),
    )
    .await;

    // Fault stop reasons are NOT eligible for hold — plain label emitted
    assert_eq!(
        stopped.reason.as_deref(),
        Some("stale_telemetry"),
        "fault stop must hard-stop even when stop_behavior=hold"
    );
}

#[tokio::test]
async fn hold_behavior_not_homed_falls_back_to_hard_stop() {
    let (state, _dir) = common::make_state();
    // Force InBand instead of Homed
    {
        use cortex::boot_state::BootState;
        let mut bs = state.boot_state.write().expect("boot_state");
        let inv = state.inventory.read().expect("inv");
        for m in inv.actuators() {
            bs.insert(m.common.role.clone(), BootState::InBand);
        }
    }
    common::seed_feedback(&state);

    let role = "right_arm.shoulder_roll";
    set_travel_limits(&state, role, -0.5, 0.5);
    set_stop_behavior(&state, role, cortex::motion::StopBehavior::Hold);

    // InBand doesn't permit enable, so start won't pass preflight with
    // require_verified. Override to allow motion start for the test.
    {
        use cortex::boot_state::BootState;
        let mut bs = state.boot_state.write().expect("boot_state");
        bs.insert(role.into(), BootState::Homed);
    }

    let mut status_rx = state.motion_status_tx.subscribe();

    let run_id = state
        .motion
        .start(
            &state,
            role,
            MotionIntent::Sweep {
                speed_rad_s: 0.1,
                turnaround_rad: 0.05,
            },
        )
        .await
        .expect("start");

    let _ = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Running,
        Duration::from_secs(1),
    )
    .await;

    // Now downgrade boot state to InBand mid-run — won't trip preflight
    // (preflight checks BootNotReady which is different), but stop policy
    // will see non-Homed and fall back to hard stop.
    {
        use cortex::boot_state::BootState;
        let mut bs = state.boot_state.write().expect("boot_state");
        bs.insert(role.into(), BootState::InBand);
    }

    assert!(state.motion.stop(role).await);

    let stopped = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Stopped,
        Duration::from_secs(2),
    )
    .await;

    // Not homed → hard stop, so plain "operator" label
    assert_eq!(
        stopped.reason.as_deref(),
        Some("operator"),
        "non-homed actuator should hard-stop even with stop_behavior=hold"
    );
}

#[tokio::test]
async fn mit_backend_hold_on_stop_emits_hold_reason() {
    let (state, _dir) = common::make_state();
    {
        let mut effective = state.effective.write().expect("effective poisoned");
        effective.safety.motion_backend = MotionBackend::Mit;
    }
    common::force_homed(&state);
    common::seed_feedback(&state);

    let role = "right_arm.shoulder_roll";
    set_travel_limits(&state, role, -0.5, 0.5);
    set_stop_behavior(&state, role, cortex::motion::StopBehavior::Hold);

    let mut status_rx = state.motion_status_tx.subscribe();

    let run_id = state
        .motion
        .start(
            &state,
            role,
            MotionIntent::Sweep {
                speed_rad_s: 0.1,
                turnaround_rad: 0.05,
            },
        )
        .await
        .expect("start");

    let _ = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Running,
        Duration::from_secs(1),
    )
    .await;

    assert!(state.motion.stop(role).await);

    let stopped = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Stopped,
        Duration::from_secs(2),
    )
    .await;

    assert_eq!(
        stopped.reason.as_deref(),
        Some("operator_hold"),
        "MIT backend should also respect stop_behavior=hold"
    );
}

#[tokio::test]
async fn canonical_shoulder_roll_hold_without_explicit_stop_behavior() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);

    let role = common::RIGHT_ARM_SHOULDER_ROLL;
    set_travel_limits(&state, role, -0.5, 0.5);
    set_stop_behavior(&state, role, cortex::motion::StopBehavior::HardStop);
    // joint_kind=shoulder_roll still makes effective_behavior() return Hold.

    let mut status_rx = state.motion_status_tx.subscribe();

    let run_id = state
        .motion
        .start(
            &state,
            role,
            MotionIntent::Sweep {
                speed_rad_s: 0.1,
                turnaround_rad: 0.05,
            },
        )
        .await
        .expect("start");

    let _ = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Running,
        Duration::from_secs(1),
    )
    .await;

    assert!(state.motion.stop(role).await);

    let stopped = wait_for_state(
        &mut status_rx,
        &run_id,
        MotionState::Stopped,
        Duration::from_secs(2),
    )
    .await;

    assert_eq!(
        stopped.reason.as_deref(),
        Some("operator_hold"),
        "canonical shoulder_roll should hold on graceful stop via joint_kind default"
    );
}
