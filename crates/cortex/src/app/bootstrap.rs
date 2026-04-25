//! Daemon startup: config, tracing, `AppState`, HTTP + WebTransport runners.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::broadcast;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, reload, EnvFilter, Registry};

use crate::settings::data;
use crate::{
    audit, can, config, deployment, http, inventory, log_layer, log_store, reminders, settings,
    spec, state, system, telemetry, types::LogEntry, wt,
};

/// Run the cortex daemon: same behavior as the binary `main`, but callable with
/// an explicit argv (e.g. integration tests).
pub async fn run(args: Vec<String>) -> Result<()> {
    // Phase 1: read CLI + config WITHOUT a tracing subscriber installed
    // yet. The config tells us where the log store lives and what filter
    // directive to bring up; both are needed before we can build the
    // subscriber that captures into the store. Errors here go straight
    // to stderr via `eprintln!` because tracing isn't up.
    let config_path = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("./config/cortex.toml");

    let cfg = config::Config::load(config_path)
        .with_context(|| format!("loading config from {config_path}"))?;

    // Build a reload-able EnvFilter so PUT /api/logs/level can swap it
    // at runtime without restarting. Precedence on first boot:
    //   1. RUST_LOG env (developer override, matches every other Rust
    //      daemon's convention so tests can inject `RUST_LOG=debug`),
    //   2. .cortex/log_filter.txt (operator's last accepted PUT, restored
    //      so a verbose-debug session survives a daemon bounce),
    //   3. config [logs].default_filter (the safe baseline).
    let persisted_filter = read_persisted_filter(&cfg);
    let initial_filter_str = std::env::var("RUST_LOG")
        .ok()
        .or(persisted_filter)
        .unwrap_or_else(|| cfg.logs.default_filter.clone());
    let initial_filter = EnvFilter::try_new(&initial_filter_str)
        .unwrap_or_else(|_| EnvFilter::new(&cfg.logs.default_filter));
    let (filter_layer, filter_handle) = reload::Layer::new(initial_filter);

    // Open the persistent store + create the live broadcast that the
    // capture layer will tee into. The store's parent dir is created
    // here (in addition to LogStore::open's own create_dir_all) so any
    // permission failure surfaces in the operator's terminal instead
    // of disappearing into the very first dropped log line.
    if let Some(parent) = cfg.logs.db_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating log_store parent dir {}", parent.display()))?;
        }
    }
    let log_store_handle = log_store::LogStore::open(&cfg.logs)
        .with_context(|| format!("opening log store at {}", cfg.logs.db_path.display()))?;
    let (log_event_tx, _) = broadcast::channel::<LogEntry>(2048);
    let capture_layer =
        log_layer::LogCaptureLayer::new(log_store_handle.clone(), log_event_tx.clone());

    Registry::default()
        .with(filter_layer)
        .with(fmt::layer().with_target(true))
        .with(capture_layer)
        .init();

    info!("loaded config from {config_path}");

    // rustls 0.23 mandates an explicit CryptoProvider when more than one
    // could be selected. We compile rustls with `features = ["ring"]` (see
    // crates/cortex/Cargo.toml for why ring over aws-lc-rs); without this
    // call both the axum-server HTTPS listener and the wtransport endpoint
    // panic on first use. Must run before any TLS object is built.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls ring CryptoProvider");

    let actuators_dir = cfg.paths.actuator_spec.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "actuator_spec path {:?} has no parent directory",
            cfg.paths.actuator_spec
        )
    })?;
    let specs = spec::load_robstride_specs(actuators_dir, Some(&cfg.paths.actuator_spec))
        .with_context(|| format!("loading RobStride specs under {:?}", actuators_dir))?;
    let mut loaded_models: Vec<&'static str> = specs.keys().map(|m| m.as_spec_label()).collect();
    loaded_models.sort();
    info!(?loaded_models, "loaded RobStride actuator specs");

    // First-boot bootstrap on the Pi: the release tarball ships a baseline
    // inventory in the read-only `/opt/rudy/...` tree, while cortex reads
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

    let settings_init = if cfg.runtime.enabled {
        Some(
            settings::init(&cfg)
                .context("open runtime settings DB (see [runtime] in cortex.toml)")?,
        )
    } else {
        None
    };

    let inv = load_bootstrap_inventory(&cfg, settings_init.as_ref())
        .with_context(|| format!("loading inventory {:?}", cfg.paths.inventory))?;

    for d in &inv.devices {
        if let inventory::Device::Actuator(a) = d {
            let m = a.robstride_model();
            if !specs.contains_key(&m) {
                anyhow::bail!(
                    "inventory actuator {:?} requires spec for model {}, but no robstride_{}.yaml was loaded from {:?}",
                    a.common.role,
                    m.as_spec_label(),
                    m.robstride_yaml_suffix(),
                    actuators_dir
                );
            }
        }
    }

    info!(
        devices = inv.devices.len(),
        actuators = inv.actuators().count(),
        "loaded inventory"
    );

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

    // The log_event broadcast sender we hand to AppState MUST be the
    // same one the LogCaptureLayer is publishing to, otherwise the WT
    // router (which subscribes to `state.log_event_tx`) would never see
    // a single event. `new_with_log_tx` is the seam that ensures both
    // sides hold clones of the same channel.
    let app_state = Arc::new(state::AppState::new_with_log_tx(
        cfg.clone(),
        settings_init,
        specs,
        inv,
        audit,
        real_can,
        reminder_store,
        log_event_tx,
    ));

    // Wire the persistent store onto AppState so API handlers (and
    // AuditLog::write's fan-out path) can reach it.
    app_state.attach_log_store(log_store_handle);

    // Wire the runtime filter mutator. We can't store the
    // `reload::Handle` directly on `AppState` because its second type
    // parameter is the Registry's stacked-layer type (long, painful to
    // spell, and changes any time we add a layer); instead we hand
    // AppState a closure that owns the handle and exposes only the
    // `modify` operation the API needs.
    let filter_setter: state::FilterReloadFn = {
        let handle = filter_handle.clone();
        Arc::new(move |new_filter| {
            handle
                .modify(move |f| *f = new_filter)
                .map_err(|e| format!("filter reload failed: {e}"))
        })
    };
    app_state.attach_filter_reload(filter_setter);

    can::spawn(app_state.clone())?;

    if app_state.read_effective().safety.scan_on_boot
        && !app_state.cfg.can.mock
        && app_state.real_can.is_some()
    {
        let st = app_state.clone();
        tokio::spawn(async move {
            let join = tokio::task::spawn_blocking(move || {
                can::hardware_active_scan(&st, None, 1, 0x7F, Duration::from_millis(50))
            })
            .await;
            match join {
                Ok(Ok(report)) => {
                    info!(
                        discovered = report.discovered.len(),
                        attempts = report.attempts.len(),
                        "boot-time hardware scan finished"
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "boot-time hardware scan failed");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "boot-time hardware scan task panicked");
                }
            }
        });
    }

    telemetry::spawn(app_state.clone());
    system::spawn(app_state.clone());
    deployment::spawn(app_state.clone());

    let (http_ready_tx, http_ready_rx) = tokio::sync::oneshot::channel();
    let mut http_handle = tokio::spawn(http::run(app_state.clone(), Some(http_ready_tx)));
    let mut wt_handle = tokio::spawn(wt::run(app_state.clone()));

    match tokio::time::timeout(Duration::from_secs(5), http_ready_rx).await {
        Ok(Ok(())) => notify_systemd_ready(),
        Ok(Err(_)) => tracing::warn!("http ready signal dropped before startup"),
        Err(_) => tracing::warn!("timed out waiting for http startup ready signal"),
    }
    spawn_systemd_watchdog_task();

    info!("cortex is up");

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

#[cfg(target_os = "linux")]
fn notify_systemd_ready() {
    if let Err(e) = sd_notify::notify(false, &[sd_notify::NotifyState::Ready]) {
        tracing::warn!(error = %e, "failed to send systemd READY notification");
    }
}

#[cfg(not(target_os = "linux"))]
fn notify_systemd_ready() {}

#[cfg(target_os = "linux")]
fn spawn_systemd_watchdog_task() {
    let Some(interval) = watchdog_interval_from_env() else {
        return;
    };
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            if let Err(e) = sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog]) {
                tracing::warn!(error = %e, "failed to send systemd watchdog ping");
                break;
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
fn spawn_systemd_watchdog_task() {}

#[cfg(target_os = "linux")]
fn watchdog_interval_from_env() -> Option<Duration> {
    let usec = std::env::var("WATCHDOG_USEC").ok()?.parse::<u64>().ok()?;
    if usec == 0 {
        return None;
    }
    Some(Duration::from_micros((usec / 2).max(1)))
}

/// Read the operator's last accepted log filter directive from
/// `<audit_dir>/log_filter.txt`. Returns `None` if the file is missing,
/// unreadable, or empty after trim. PUT /api/logs/level writes this file
/// on every successful change so the operator's chosen verbosity
/// survives a daemon restart (otherwise debug-tracing a flaky CAN
/// session would silently revert at the next bounce).
fn load_bootstrap_inventory(
    cfg: &config::Config,
    settings: Option<&settings::RuntimeConfigInit>,
) -> Result<inventory::Inventory> {
    if let Some(s) = settings {
        if let Some(db) = &s.db {
            let c = db
                .lock()
                .map_err(|e| anyhow::anyhow!("runtime db lock: {e}"))?;
            if let Some(body) = data::get_inventory_json(&*c).context("read inventory_doc")? {
                if !body.is_empty() {
                    let inv: inventory::Inventory = serde_json::from_str(&body)
                        .context("deserialize inventory from runtime db")?;
                    inv.validate()
                        .context("inventory from DB failed validate()")?;
                    return Ok(inv);
                }
            }
        }
    }

    let inv =
        inventory::Inventory::load(&cfg.paths.inventory).context("load inventory from yaml")?;

    if let Some(s) = settings {
        if let Some(db) = &s.db {
            let json = serde_json::to_string(&inv).context("inventory json for db seed")?;
            let c = db
                .lock()
                .map_err(|e| anyhow::anyhow!("runtime db lock: {e}"))?;
            if data::get_inventory_json(&*c)
                .context("get inventory after yaml load")?
                .is_none()
            {
                data::set_inventory_json(&*c, &json).context("seed inventory_doc from yaml")?;
            }
        }
    }

    Ok(inv)
}

fn read_persisted_filter(cfg: &config::Config) -> Option<String> {
    let path = cfg
        .paths
        .audit_log
        .parent()
        .map(|p| p.join("log_filter.txt"))?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
