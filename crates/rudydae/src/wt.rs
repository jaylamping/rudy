//! WebTransport listener.
//!
//! Phase 1: accepts sessions, validates the `?token=` query param, and
//! broadcasts per-motor feedback as CBOR datagrams. The client-side
//! subscription protocol (bidi stream, `WtSubscribe` messages) is specified
//! in `types::WtSubscribe` and will be parsed here in Phase 2 to selectively
//! enable faults + logs streams.
//!
//! When `cfg.webtransport.enabled = false` or TLS is not configured, this
//! function logs a note and returns `Ok(())` so it's safe to `tokio::spawn`
//! unconditionally from main.

use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, info, warn};

use crate::auth;
use crate::state::SharedState;
use crate::types::MotorFeedback;

pub async fn run(state: SharedState) -> Result<()> {
    if !state.cfg.webtransport.enabled {
        info!("webtransport listener disabled");
        return Ok(());
    }

    if !state.cfg.http.tls.enabled {
        warn!("webtransport.enabled=true but http.tls.enabled=false; WebTransport requires TLS. Disabling WT listener.");
        return Ok(());
    }

    let cert_path = state
        .cfg
        .http
        .tls
        .cert_path
        .clone()
        .context("http.tls.cert_path required for WebTransport")?;
    let key_path = state
        .cfg
        .http
        .tls
        .key_path
        .clone()
        .context("http.tls.key_path required for WebTransport")?;

    let bind: std::net::SocketAddr = state.cfg.webtransport.bind.parse().with_context(|| {
        format!(
            "parsing webtransport.bind {:?}",
            state.cfg.webtransport.bind
        )
    })?;

    let config = wtransport::ServerConfig::builder()
        .with_bind_address(bind)
        .with_identity(
            wtransport::Identity::load_pemfiles(&cert_path, &key_path)
                .await
                .context("loading WT cert/key")?,
        )
        .keep_alive_interval(Some(Duration::from_secs(3)))
        .build();

    let server = wtransport::Endpoint::server(config).context("binding WT endpoint")?;
    info!(%bind, "webtransport listener up");

    loop {
        let incoming = server.accept().await;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_session(incoming, state).await {
                warn!("wt session error: {e:#}");
            }
        });
    }
}

async fn handle_session(
    incoming: wtransport::endpoint::IncomingSession,
    state: SharedState,
) -> Result<()> {
    let session_req = incoming
        .await
        .context("awaiting WebTransport session request")?;

    let path = session_req.path().to_string();
    let token = extract_query_param(&path, "token");

    if !auth::verify_wt_token(&state, token.as_deref()) {
        warn!(path = %path, "wt: rejecting session (bad token)");
        session_req.not_found().await;
        return Ok(());
    }

    let connection = session_req.accept().await.context("accepting WT session")?;
    info!(
        "wt: session accepted from {:?}",
        connection.remote_address()
    );

    let mut rx = state.feedback_tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(fb) => {
                let mut buf = Vec::with_capacity(128);
                if ciborium::into_writer(&fb, &mut buf).is_ok() {
                    if let Err(e) = connection.send_datagram(buf) {
                        debug!("wt: datagram send failed: {e}; closing session");
                        break;
                    }
                }
            }
            Err(RecvError::Lagged(n)) => {
                debug!("wt: feedback receiver lagged {n}");
                continue;
            }
            Err(RecvError::Closed) => break,
        }
    }

    // Silence unused-type warning until Phase 2 wires the subscribe protocol.
    let _: Option<MotorFeedback> = None;

    Ok(())
}

fn extract_query_param(path: &str, key: &str) -> Option<String> {
    let (_, query) = path.split_once('?')?;
    for kv in query.split('&') {
        if let Some((k, v)) = kv.split_once('=') {
            if k == key {
                // Raw value - operators should pick token characters that
                // don't need URL encoding.
                return Some(v.to_owned());
            }
        }
    }
    None
}
