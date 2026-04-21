//! WebTransport listener.
//!
//! Accepts QUIC sessions and hands each one off to `wt_router::run_session`,
//! which is where all the per-session state (subscription filter, sequence
//! counters, reliable-stream handle) lives. This module's only jobs:
//!
//! 1. Honor `cfg.webtransport.enabled` + cert/key configuration. If TLS
//!    materials are missing, log and return cleanly so the daemon stays up.
//! 2. Bind the QUIC endpoint and accept loop.
//! 3. Spawn one `run_session` per accepted connection.
//!
//! Wire format reference: see `types::WtEnvelope`. Every datagram and every
//! reliable-stream frame is `WtEnvelope<Payload>` encoded as CBOR. Reliable
//! frames are length-prefixed (u32 BE) inside the QUIC stream.
//!
//! Adding a new stream type does NOT require editing this file. See
//! `types::declare_wt_streams!` for the recipe.
//!
//! Unlike the REST listener (which is plaintext fronted by `tailscale serve`),
//! WebTransport terminates TLS itself: `tailscale serve` is HTTP/1.1+HTTP/2
//! only, so it cannot proxy HTTP/3 / QUIC. The WT endpoint therefore loads the
//! same Tailscale Let's Encrypt cert directly. See ADR-0004 addendum.

use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::state::SharedState;

use super::session;

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

    session::run_session(connection, state).await
}
