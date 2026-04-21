use super::*;

fn err(msg: &str) -> anyhow::Error {
    anyhow::anyhow!("{msg}")
}

#[test]
fn fresh_role_polls_immediately() {
    let b = MotorBackoff::new();
    assert!(b.should_poll("a"));
}

#[test]
fn initial_backoff_matches_100hz_poll_tick() {
    assert_eq!(INITIAL_BACKOFF, Duration::from_millis(10));
}

#[test]
fn first_failure_starts_initial_backoff() {
    let b = MotorBackoff::new();
    let t0 = Instant::now();

    b.record_failure_at("a", &err("boom"), t0);

    assert!(!b.should_poll_at("a", t0));
    assert!(!b.should_poll_at("a", t0 + INITIAL_BACKOFF - Duration::from_millis(1)));
    assert!(b.should_poll_at("a", t0 + INITIAL_BACKOFF));
}

#[test]
fn backoff_doubles_per_failure() {
    let b = MotorBackoff::new();
    let t0 = Instant::now();

    b.record_failure_at("a", &err("1"), t0);
    b.record_failure_at("a", &err("2"), t0);
    // After the 2nd failure, backoff is 2 * INITIAL_BACKOFF.
    let after_2 = INITIAL_BACKOFF * 2;
    assert!(b.should_poll_at("a", t0 + after_2));
    assert!(!b.should_poll_at("a", t0 + after_2 - Duration::from_millis(1)));

    b.record_failure_at("a", &err("3"), t0);
    let after_3 = INITIAL_BACKOFF * 4;
    assert!(b.should_poll_at("a", t0 + after_3));
    assert!(!b.should_poll_at("a", t0 + after_3 - Duration::from_millis(1)));
}

#[test]
fn backoff_caps_at_max() {
    let b = MotorBackoff::new();
    let t0 = Instant::now();

    // 10, 20, 40, ... ms (doubling) until the 30 s cap.
    for _ in 0..20 {
        b.record_failure_at("a", &err("x"), t0);
    }

    assert!(b.should_poll_at("a", t0 + MAX_BACKOFF));
    assert!(!b.should_poll_at("a", t0 + MAX_BACKOFF - Duration::from_millis(1)));
}

#[test]
fn success_resets_state_and_resumes_immediate_polling() {
    let b = MotorBackoff::new();
    let t0 = Instant::now();

    b.record_failure_at("a", &err("flaky"), t0);
    b.record_failure_at("a", &err("flaky"), t0);
    assert!(!b.should_poll_at("a", t0 + Duration::from_millis(1)));

    b.record_success("a");
    assert!(b.should_poll_at("a", t0 + Duration::from_millis(1)));
    assert!(b.should_poll_at("a", t0 + MAX_BACKOFF + Duration::from_secs(60)));
}

#[test]
fn per_motor_state_is_independent() {
    let b = MotorBackoff::new();
    let t0 = Instant::now();

    b.record_failure_at("a", &err("a-down"), t0);
    // "b" has no recorded failures, so it should poll freely even
    // while "a" is in cooldown.
    assert!(!b.should_poll_at("a", t0));
    assert!(b.should_poll_at("b", t0));
}
