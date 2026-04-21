//! Per-motor poll backoff for the real-CAN telemetry loop.
//!
//! Background: when a motor is in inventory but unreachable on the bus
//! (powered off, harness unplugged, controller in BUS-OFF, no peer to ACK
//! frames), every read attempt either times out (~5–30 ms) or queues a
//! frame that will never be ACK'd. Polling that motor at 10 Hz piles up
//! work the kernel can't drain — eventually `write` returns ENOBUFS
//! (errno 105) and we spam the journal with one warning per tick. Worse,
//! `LinuxCanCore::poll_once` short-circuits on the first per-motor error,
//! so a single flaky motor stops the *other* motors from being polled too.
//!
//! This module provides a per-motor exponential backoff state. The poll
//! loop calls [`MotorBackoff::should_poll`] before each motor's read; if
//! it returns `false`, the motor is skipped for this tick. After the
//! read completes the loop calls either [`MotorBackoff::record_success`]
//! (resets the backoff and emits a one-shot "recovered" log on transition)
//! or [`MotorBackoff::record_failure`] (doubles the wait, capped at
//! `MAX_BACKOFF`, and emits sparse logs on state transitions).
//!
//! Logs are deliberately sparse: full warning on the first failure,
//! `info!("recovered")` on the first success after a failure streak, and
//! `debug!` for individual retries. The previous implementation produced
//! 10 WARN/sec under fault; we target one WARN per fault transition.
//!
//! Backoff state lives inside `LinuxCanCore` so it survives across
//! `poll_once` calls (each call is a fresh `tokio::task::spawn_blocking`).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

/// First retry delay after a failure. Starts here, doubles each subsequent
/// failure, capped at [`MAX_BACKOFF`]. Picked to match the typical poll
/// cadence (`telemetry.poll_interval_ms` defaults to 16 ms ≈ 60 Hz) so
/// the very first retry happens on the next tick — only persistent
/// failures back off.
const INITIAL_BACKOFF: Duration = Duration::from_millis(16);

/// Cap on the per-motor retry interval. Once a motor has been failing for
/// long enough to hit this, we keep retrying every 30 s so a motor that
/// gets re-plugged or re-powered comes back within ~half a minute without
/// requiring a daemon restart.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Per-motor state: tracks consecutive failure count and the next time
/// we're allowed to retry. `None` next_retry means "no failures, poll
/// freely".
#[derive(Debug, Clone)]
struct MotorState {
    consecutive_failures: u32,
    next_retry: Option<Instant>,
    current_backoff: Duration,
}

impl Default for MotorState {
    fn default() -> Self {
        Self {
            consecutive_failures: 0,
            next_retry: None,
            current_backoff: INITIAL_BACKOFF,
        }
    }
}

#[derive(Debug, Default)]
pub struct MotorBackoff {
    motors: Mutex<HashMap<String, MotorState>>,
}

impl MotorBackoff {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if this motor's cooldown (if any) has elapsed and
    /// the caller should attempt a read. Returns `false` if the motor is
    /// still cooling down from a recent failure, in which case the caller
    /// should skip it for this tick.
    pub fn should_poll(&self, role: &str) -> bool {
        self.should_poll_at(role, Instant::now())
    }

    /// Test seam for [`Self::should_poll`] that takes the current time
    /// explicitly so unit tests don't have to sleep.
    fn should_poll_at(&self, role: &str, now: Instant) -> bool {
        let motors = self.motors.lock().expect("backoff mutex poisoned");
        match motors.get(role) {
            None => true,
            Some(s) => match s.next_retry {
                None => true,
                Some(t) => now >= t,
            },
        }
    }

    /// Record a successful read for `role`. If this success ended a
    /// failure streak, logs a single `info!` "recovered" message; the
    /// caller then resumes normal-cadence polling.
    pub fn record_success(&self, role: &str) {
        let mut motors = self.motors.lock().expect("backoff mutex poisoned");
        if let Some(s) = motors.get_mut(role) {
            if s.consecutive_failures > 0 {
                info!(
                    role = role,
                    failed_polls = s.consecutive_failures,
                    "real-CAN motor recovered"
                );
            }
            *s = MotorState::default();
        }
    }

    /// Record a failed read for `role` and schedule the next retry.
    /// Logs strategy:
    ///
    ///   * 1st failure  -> `WARN` with the error string (matches the
    ///     historical "first failure" signal so existing alerts still
    ///     fire),
    ///   * subsequent   -> `DEBUG` with the new backoff so the journal
    ///     stays clean under sustained outages.
    pub fn record_failure(&self, role: &str, error: &anyhow::Error) {
        self.record_failure_at(role, error, Instant::now())
    }

    fn record_failure_at(&self, role: &str, error: &anyhow::Error, now: Instant) {
        let mut motors = self.motors.lock().expect("backoff mutex poisoned");
        let s = motors.entry(role.into()).or_default();

        s.consecutive_failures = s.consecutive_failures.saturating_add(1);

        if s.consecutive_failures == 1 {
            warn!(
                role = role,
                error = ?error,
                "real-CAN telemetry poll failed; backing off"
            );
            s.current_backoff = INITIAL_BACKOFF;
        } else {
            s.current_backoff = (s.current_backoff * 2).min(MAX_BACKOFF);
            debug!(
                role = role,
                consecutive_failures = s.consecutive_failures,
                next_backoff_ms = s.current_backoff.as_millis() as u64,
                "real-CAN poll still failing; backing off further"
            );
        }

        s.next_retry = Some(now + s.current_backoff);
    }
}

#[cfg(test)]
#[path = "backoff_tests.rs"]
mod tests;
