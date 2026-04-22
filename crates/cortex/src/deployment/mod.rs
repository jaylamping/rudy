//! Poll GitHub `latest.json`, `systemctl` updater state, and `current.sha` mtime
//! into [`AppState::deployment`][0].
//!
//! [0]: crate::app::state::AppState

use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Command;
use std::time::Duration;

use chrono::DateTime;
use reqwest::Client;
use serde::Deserialize;
use tracing::warn;

use crate::app::state::SharedState;
use crate::types::{BuildIdentity, ChannelLatest, DeploymentInfo, UpdaterStatus};

const DEFAULT_LATEST_URL: &str =
    "https://github.com/jaylamping/rudy/releases/latest/download/latest.json";

const ENV_LATEST_URL: &str = "RUDY_GITHUB_LATEST_URL";
const ENV_CURRENT_SHA: &str = "RUDY_CURRENT_SHA_FILE";
pub const DEFAULT_CURRENT_SHA_FILE: &str = "/opt/rudy/current.sha";

const POLL_SECS: u64 = 60;

pub fn manifest_url() -> String {
    std::env::var(ENV_LATEST_URL)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_LATEST_URL.to_string())
}

fn current_sha_path() -> PathBuf {
    std::env::var(ENV_CURRENT_SHA)
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CURRENT_SHA_FILE))
}

/// Spawn a task that refreshes every [`POLL_SECS`]. The first `interval.tick` completes
/// immediately, so the first fetch runs as soon as the task starts.
pub fn spawn(state: SharedState) {
    tokio::spawn(async move {
        let client = match Client::builder()
            .timeout(Duration::from_secs(12))
            .user_agent(concat!("cortex/", env!("CARGO_PKG_VERSION")))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "reqwest::Client::build failed; deployment poller not running");
                return;
            }
        };
        let url = manifest_url();
        let mut tick = tokio::time::interval(Duration::from_secs(POLL_SECS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = refresh_once(&state, &client, &url).await {
                warn!(error = %e, "deployment refresh failed");
            }
        }
    });
}

async fn refresh_once(state: &SharedState, client: &Client, url: &str) -> Result<(), String> {
    let build = BuildIdentity::from_build_env();
    let mut latest = fetch_latest_json(client, url).await;
    if latest.manifest_error.is_none() && latest.commit_sha.is_none() {
        latest.manifest_error = Some("latest.json: missing commit_sha".to_string());
    }
    let latest_manifest_ok = latest.commit_sha.is_some() && latest.manifest_error.is_none();
    let is_stale = latest_manifest_ok
        && build.commit_sha != "unknown"
        && latest
            .commit_sha
            .as_deref()
            .is_some_and(|s| s != build.commit_sha.as_str());
    let updater = probe_updater().await;
    let snap = DeploymentInfo {
        build,
        latest,
        is_stale,
        latest_manifest_ok,
        updater,
    };
    *state.deployment.write().await = snap;
    Ok(())
}

async fn fetch_latest_json(client: &Client, url: &str) -> ChannelLatest {
    match client.get(url).send().await {
        Err(e) => ChannelLatest {
            commit_sha: None,
            short_sha: None,
            built_at: None,
            manifest_error: Some(e.to_string()),
        },
        Ok(resp) => {
            if !resp.status().is_success() {
                return ChannelLatest {
                    commit_sha: None,
                    short_sha: None,
                    built_at: None,
                    manifest_error: Some(format!("HTTP {} from {}", resp.status(), url)),
                };
            }
            let bytes = match resp.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    return ChannelLatest {
                        commit_sha: None,
                        short_sha: None,
                        built_at: None,
                        manifest_error: Some(e.to_string()),
                    };
                }
            };
            let parsed: LatestManifest = match serde_json::from_slice(&bytes) {
                Ok(m) => m,
                Err(e) => {
                    return ChannelLatest {
                        commit_sha: None,
                        short_sha: None,
                        built_at: None,
                        manifest_error: Some(e.to_string()),
                    };
                }
            };
            ChannelLatest {
                commit_sha: Some(parsed.commit_sha),
                short_sha: if parsed.short_sha.is_empty() {
                    None
                } else {
                    Some(parsed.short_sha)
                },
                built_at: if parsed.built_at.is_empty() {
                    None
                } else {
                    Some(parsed.built_at)
                },
                manifest_error: None,
            }
        }
    }
}

#[derive(Deserialize)]
struct LatestManifest {
    commit_sha: String,
    #[serde(default)]
    short_sha: String,
    #[serde(default)]
    built_at: String,
}

async fn probe_updater() -> UpdaterStatus {
    let path = current_sha_path();
    match tokio::task::spawn_blocking(move || probe_updater_inner(&path)).await {
        Ok(s) => s,
        Err(_) => UpdaterStatus::not_probed(),
    }
}

fn probe_updater_inner(current_sha: &Path) -> UpdaterStatus {
    let last_applied = file_mtime_rfc3339(current_sha);
    #[cfg(target_os = "linux")]
    {
        let show = match Command::new("systemctl")
            .args([
                "show",
                "cortex-update.timer",
                "-p",
                "LoadState",
                "-p",
                "LastTriggerUSec",
                "--no-pager",
            ])
            .output()
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(_) => {
                return non_linuxish(last_applied);
            }
        };
        if show.contains("LoadState=not-found") {
            return non_linuxish(last_applied);
        }
        if show.trim().is_empty() {
            return non_linuxish(last_applied);
        }
        let last_check = show
            .lines()
            .find(|l| l.starts_with("LastTriggerUSec="))
            .and_then(|l| l.strip_prefix("LastTriggerUSec="))
            .map(str::trim)
            .filter(|s| *s != "n/a" && !s.is_empty())
            .map(str::to_string);
        let ta = is_active("cortex-update.timer");
        let failed = is_failed("cortex-update.service");
        let healthy = ta == Some(true) && !failed.unwrap_or(false);
        UpdaterStatus {
            systemd_probed: true,
            last_check,
            last_applied,
            timer_active: Some(ta.unwrap_or(false)),
            service_failed: Some(failed.unwrap_or(false)),
            healthy,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = current_sha;
        non_linuxish(last_applied)
    }
}

/// Linux units missing, or a dev host: keep `current.sha` mtime when present.
fn non_linuxish(last_applied: Option<String>) -> UpdaterStatus {
    UpdaterStatus {
        systemd_probed: false,
        last_check: None,
        last_applied,
        timer_active: None,
        service_failed: None,
        healthy: true,
    }
}

/// Exit 0 and stdout `active` means active. Otherwise inactive / missing — `false`.
#[cfg(target_os = "linux")]
fn is_active(unit: &str) -> Option<bool> {
    let o = Command::new("systemctl")
        .args(["is-active", unit])
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&o.stdout);
    let s = line.trim();
    if o.status.success() {
        return Some(s == "active");
    }
    if s == "inactive" || s == "failed" {
        return Some(false);
    }
    Some(false)
}

/// Exit 0 if the unit is in the **failed** state.
#[cfg(target_os = "linux")]
fn is_failed(unit: &str) -> Option<bool> {
    let o = Command::new("systemctl")
        .args(["is-failed", unit])
        .output()
        .ok()?;
    Some(o.status.success())
}

fn file_mtime_rfc3339(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let t = meta.modified().ok()?;
    let dt: DateTime<chrono::Utc> = t.into();
    Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
