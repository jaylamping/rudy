//! Pin the telemetry pipeline that feeds the WebTransport firehose.
//!
//! `wt::handle_session` subscribes to `state.feedback_tx` and forwards every
//! `MotorFeedback` it receives. If the mock CAN core (or, later, the real
//! Linux SocketCAN core) stops broadcasting on this channel, the entire
//! WebTransport telemetry stream goes silent — even though connections still
//! succeed and the codec test still passes.
//!
//! This test asserts the channel actually fires for every motor in inventory
//! within a few mock ticks. We do **not** test the WebTransport transport
//! layer itself; that's intentionally out of scope (see request thread).

use std::collections::HashSet;
use std::time::Duration;

use rudydae::can;

mod common;

#[tokio::test]
async fn mock_can_publishes_feedback_for_every_inventoried_motor() {
    let (state, _dir) = common::make_state();
    let expected_roles: HashSet<String> = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .motors
        .iter()
        .map(|m| m.role.clone())
        .collect();

    // Subscribe BEFORE spawning the producer to guarantee no missed messages
    // (broadcast::Sender drops messages when no receiver exists).
    let mut rx = state.feedback_tx.subscribe();

    can::spawn(state.clone()).expect("spawn mock CAN");

    // Mock CAN ticks at telemetry.poll_interval_ms (10 ms in the test fixture)
    // and emits one frame per motor per tick. Two motors × ~5 ticks = 10
    // frames; cap the wait at 1 s so a stalled producer fails fast.
    let mut seen: HashSet<String> = HashSet::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    while seen != expected_roles {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!(
                "mock CAN never produced feedback for all motors; saw {seen:?}, expected {expected_roles:?}"
            );
        }
        let fb = match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(fb)) => fb,
            Ok(Err(e)) => panic!("broadcast channel closed unexpectedly: {e}"),
            Err(_) => panic!(
                "timed out waiting for mock CAN tick; saw {seen:?}, expected {expected_roles:?}"
            ),
        };
        seen.insert(fb.role);
    }

    // Also verify the side channel (`state.latest`) is being mirrored — this
    // is what the REST `/api/motors/:role/feedback` endpoint reads from, so
    // both the WT firehose AND the REST polling fallback need it populated.
    let latest = state.latest.read().expect("latest");
    for role in &expected_roles {
        assert!(
            latest.contains_key(role),
            "AppState.latest should mirror feedback_tx; missing {role}"
        );
    }
}
