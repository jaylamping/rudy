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

#[tokio::test]
async fn sweep_lifecycle_running_then_operator_stop() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);

    let role = "shoulder_actuator_a";

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
        Some("operator"),
        "stop reason should be operator-initiated"
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

    let role = "shoulder_actuator_a";
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
                assert_eq!(f.reason.as_deref(), Some("superseded"));
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
