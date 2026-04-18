//! Regression: the daemon must stay up when the WebTransport task returns
//! `Ok(())` (which happens on every dev run, since `webtransport.enabled =
//! false` in `config/rudyd.toml`).
//!
//! Before the fix to `src/main.rs`, the top-level `tokio::select!` watched
//! both the HTTP and WT join handles and exited as soon as *either* completed.
//! `wt::run` returns immediately when WT is disabled, so the HTTP listener
//! was getting torn down within microseconds of "rudydae is up" being logged.
//! The frontend smoke script (`link/scripts/smoke-contract.mjs`) caught this
//! by failing to reach the HTTP listener at all.
//!
//! We can't exercise `main` directly (it's the binary entrypoint), but we
//! can pin the WT side of the contract: when WT is disabled, `wt::run`
//! returns `Ok(())` *immediately*, with no panic, and without leaving any
//! state behind.

use std::time::Duration;

use rudydae::wt;

mod common;

#[tokio::test]
async fn wt_run_returns_ok_immediately_when_disabled() {
    let (state, _dir) = common::make_state();
    assert!(!state.cfg.webtransport.enabled, "fixture should disable WT");

    let res = tokio::time::timeout(Duration::from_secs(2), wt::run(state.clone())).await;
    let inner = res.expect("wt::run should return well within 2s when disabled");
    inner.expect("wt::run should return Ok(()) when disabled, not bubble an error");
}

/// Sister to the above: when WT is enabled but no cert is configured,
/// `wt::run` should also bail cleanly (it logs a warning), not panic. The
/// SPA's `useWebTransport` then sees `enabled: false` from `/api/config` and
/// stays on the REST polling fallback.
#[tokio::test]
async fn wt_run_returns_ok_when_enabled_but_no_cert() {
    let (state, _dir) = common::make_state_with_wt_advert();
    assert!(state.cfg.webtransport.enabled);
    assert!(
        state.cfg.webtransport.cert_path.is_none() && state.cfg.webtransport.key_path.is_none(),
        "fixture should leave cert paths unset so wt::run hits the no-cert branch"
    );

    let res = tokio::time::timeout(Duration::from_secs(2), wt::run(state.clone())).await;
    let inner = res.expect("wt::run should return well within 2s when no cert is set");
    inner.expect("wt::run should bail cleanly when no cert is set, not bubble an error");
}
