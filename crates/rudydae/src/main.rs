//! `rudydae` - Rudy operator-console daemon.
//!
//! See docs/decisions/0004-operator-console.md for architecture + safety.

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;
use tracing_subscriber::EnvFilter;

use rudydae::{
    audit, can, config, inventory, reminders, server, spec, state, system, telemetry, wt,
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("rudydae=info,tower_http=info")),
        )
        .with_target(true)
        .init();

    // rustls 0.23 mandates an explicit CryptoProvider when more than one is
    // compiled in (we get both aws-lc-rs via wtransport and ring transitively
    // via reqwest in dev-deps). Without this both the axum-server HTTPS
    // listener and the wtransport endpoint panic on first use. Must run
    // before any TLS object is built.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("install rustls aws-lc-rs CryptoProvider");

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

    // First-boot bootstrap on the Pi: the release tarball ships a baseline
    // inventory in the read-only `/opt/rudy/...` tree, while rudydae reads
    // and writes from `/var/lib/rudy/inventory.yaml`. Copy the baseline over
    // on the very first start; afterwards the live file is the source of
    // truth and the seed is ignored.
    inventory::ensure_seeded(&cfg.paths.inventory, cfg.paths.inventory_seed.as_deref())
        .with_context(|| {
            format!(
                "seeding inventory at {:?} from {:?}",
                cfg.paths.inventory, cfg.paths.inventory_seed
            )
        })?;

    let inv = inventory::Inventory::load(&cfg.paths.inventory)
        .with_context(|| format!("loading inventory {:?}", cfg.paths.inventory))?;
    info!(motors = inv.motors.len(), "loaded inventory");

    let audit = audit::AuditLog::open(&cfg.paths.audit_log)
        .with_context(|| format!("opening audit log {:?}", cfg.paths.audit_log))?;

    // Reminders live next to the audit log so all operator-state files share
    // a parent directory and a single backup target.
    let reminders_path = cfg
        .paths
        .audit_log
        .parent()
        .map(|p| p.join("reminders.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("reminders.json"));
    let reminder_store = reminders::ReminderStore::open(&reminders_path)
        .with_context(|| format!("opening reminders {:?}", reminders_path))?;

    let real_can = can::build_handle(&cfg, &inv).context("opening CAN core")?;

    let app_state = Arc::new(state::AppState::new(
        cfg.clone(),
        spec,
        inv,
        audit,
        real_can,
        reminder_store,
    ));

    can::spawn(app_state.clone())?;
    telemetry::spawn(app_state.clone());
    system::spawn(app_state.clone());

    let mut http_handle = tokio::spawn(server::run(app_state.clone()));
    let mut wt_handle = tokio::spawn(wt::run(app_state.clone()));

    info!("rudydae is up");

    // NOTE: when webtransport.enabled = false, `wt::run` returns `Ok(())`
    // almost immediately. A naive `tokio::select!` on both handles would let
    // the daemon exit as soon as the WT task finishes — silently taking the
    // HTTP listener down with it (caught by
    // `link/scripts/smoke-contract.mjs`). So we treat a *clean* exit on
    // either side as "this surface is no longer needed" and only shut down
    // when the HTTP listener stops, an error surfaces, or Ctrl-C arrives.
    tokio::select! {
        res = &mut http_handle => {
            res??;
            info!("http listener stopped; shutting down");
        }
        res = &mut wt_handle => {
            match res? {
                Ok(()) => info!("webtransport task finished; http listener still serving"),
                Err(e) => return Err(e),
            }
            // Don't poll wt_handle again; await the rest from a smaller
            // select.
            tokio::select! {
                res = &mut http_handle => {
                    res??;
                    info!("http listener stopped; shutting down");
                }
                res = tokio::signal::ctrl_c() => {
                    res?;
                    info!("shutdown signal received");
                }
            }
        }
        res = tokio::signal::ctrl_c() => {
            res?;
            info!("shutdown signal received");
        }
    }

    Ok(())
}
