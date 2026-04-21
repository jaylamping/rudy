//! Behavior pins for the non-Linux `RealCanHandle` stub.
//!
//! On Linux, `RealCanHandle = LinuxCanCore` (the real SocketCAN path) and
//! every method talks to a CAN bus; that path is exercised by the per-bus
//! integration tests on the deployment Pi, not here. On every other host
//! (macOS / Windows dev machines) `RealCanHandle` is the stub defined in
//! `crates/cortex/src/can/mod.rs`, which `bail!`s on every method *except*
//! `read_add_offset`, which the commissioned-zero plan asks to return
//! `Ok(0.0)` so contract tests for the upcoming commission endpoint and
//! boot orchestrator don't need a real CAN bus to run.
//!
//! These tests pin that stub contract. If a future change makes the stub
//! `bail!` on `read_add_offset`, the contract tests landing in Phase B
//! (commission endpoint) and Phase C (boot orchestrator) would silently
//! drift from the documented semantics on macOS dev hosts; this test
//! catches that drift up front.

#![cfg(not(target_os = "linux"))]

use cortex::can::RealCanHandle;
use cortex::inventory::Actuator;

mod common;

/// The non-Linux stub returns `Ok(0.0)` from `read_add_offset` regardless
/// of which motor is asked about — the value isn't read from any bus,
/// it's the documented mock-CAN contract.
#[tokio::test]
async fn non_linux_read_add_offset_returns_zero() {
    let (state, _dir) = common::make_state();
    let motor: Actuator = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuators()
        .next()
        .cloned()
        .expect("test inventory must have at least one motor");

    let handle = RealCanHandle;
    let offset = handle
        .read_add_offset(&state, &motor)
        .expect("non-Linux stub must return Ok(0.0)");
    assert_eq!(
        offset, 0.0,
        "non-Linux stub contract: read_add_offset returns 0.0 unconditionally"
    );
}
