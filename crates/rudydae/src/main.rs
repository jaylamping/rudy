//! `rudydae` - Rudy operator-console daemon.
//!
//! See docs/decisions/0004-operator-console.md for architecture + safety.

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;
use tracing_subscriber::EnvFilter;

mod api;
mod audit;
mod auth;
mod can;
mod config;
mod inventory;
mod server;
mod spec;
mod state;
mod telemetry;
mod types;
mod util;
mod wt;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("rudydae=info,tower_http=info")),
        )
        .with_target(true)
        .init();

    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("./config/rudyd.toml");

    let cfg = config::Config::load(config_path)
        .with_context(|| format!("loading config from {config_path}"))?;
    info!("loaded config from {config_path}");

    let spec = spec::ActuatorSpec::load(&cfg.paths.actuator_spec)
        .with_context(|| format!("loading actuator spec {:?}", cfg.paths.actuator_spec))?;
    let inv = inventory::Inventory::load(&cfg.paths.inventory)
        .with_context(|| format!("loading inventory {:?}", cfg.paths.inventory))?;
    info!(motors = inv.motors.len(), "loaded inventory");

    let audit = audit::AuditLog::open(&cfg.paths.audit_log)
        .with_context(|| format!("opening audit log {:?}", cfg.paths.audit_log))?;

    let token = auth::load_token(&cfg.auth).context("loading auth token")?;
    let real_can = can::build_handle(&cfg, &inv).context("opening CAN core")?;

    let app_state = Arc::new(state::AppState::new(
        cfg.clone(),
        spec,
        inv,
        audit,
        token,
        real_can,
    ));

    can::spawn(app_state.clone())?;
    telemetry::spawn(app_state.clone());

    let http_handle = tokio::spawn(server::run(app_state.clone()));
    let wt_handle = tokio::spawn(wt::run(app_state.clone()));

    info!("rudydae is up");

    tokio::select! {
        res = http_handle => res??,
        res = wt_handle => res??,
        _ = tokio::signal::ctrl_c() => {
            info!("shutdown signal received");
        }
    }

    Ok(())
}
