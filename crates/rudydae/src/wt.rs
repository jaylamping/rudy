//! WebTransport listener.
//!
//! Phase 1: accepts sessions and broadcasts per-motor feedback as CBOR
//! datagrams. No auth — rudydae is tailnet/localhost-only. The client-side
//! subscription protocol (bidi stream, `WtSubscribe` messages) is specified
//! in `types::WtSubscribe` and will be parsed here in Phase 2 to selectively
//! enable faults + logs streams.
//!
//! Unlike the REST listener (which is plaintext fronted by `tailscale serve`),
//! WebTransport terminates TLS itself: `tailscale serve` is HTTP/1.1+HTTP/2
//! only, so it cannot proxy HTTP/3 / QUIC. The WT endpoint therefore loads the
//! same Tailscale Let's Encrypt cert directly. See ADR-0004 addendum.
//!
//! When `cfg.webtransport.enabled = false` or no cert is configured, this
//! function logs a note and returns `Ok(())` so it's safe to `tokio::spawn`
//! unconditionally from main.

use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, info, warn};

use crate::state::SharedState;
use crate::types::MotorFeedback;

pub async fn run(state: SharedState) -> Result<()> {
    if !state.cfg.webtransport.enabled {
        info!("webtransport listener disabled");
        return Ok(());
    }

    let (Some(cert_path), Some(key_path)) = (
        state.cfg.webtransport.cert_path.clone(),
        state.cfg.webtransport.key_path.clone(),
    ) else {
        warn!(
            "webtransport.enabled=true but webtransport.cert_path / webtransport.key_path \
             not set; WebTransport requires TLS. Disabling WT listener."
        );
        return Ok(());
    };

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
