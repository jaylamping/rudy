//! Config bootstrap types (`GET /api/config`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Filesystem paths the daemon uses for read/write (operator transparency).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ServerPaths {
    /// Path cortex reads and writes for live inventory (same file as
    /// `PUT`/`DELETE` that mutate the catalog). On the Pi this is usually
    /// `/var/lib/rudy/inventory.yaml`, not the read-only seed under
    /// `/opt/rudy/config/actuators/`.
    pub inventory: String,
}

/// Compile-time binary identity (Pi release / dev host).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct BuildIdentity {
    /// Full commit SHA of this build.
    pub commit_sha: String,
    /// 12-hex short SHA, aligned with `latest.json` / Pi tags.
    pub short_sha: String,
    /// RFC 3339 UTC from CI, `git` committer date, or `unknown`.
    pub built_at: String,
    /// `CARGO_PKG_VERSION` of the `cortex` crate.
    pub package_version: String,
}

/// Latest release from GitHub `latest.json` (if fetched). Matches `cortex-update.sh` manifest.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ChannelLatest {
    pub commit_sha: Option<String>,
    pub short_sha: Option<String>,
    pub built_at: Option<String>,
    /// Last fetch/parse error (e.g. offline); UI should treat stale check as unknown.
    pub manifest_error: Option<String>,
}

/// Pi `cortex-update` timer/service + `current.sha` mtime. Best-effort on non-Linux.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct UpdaterStatus {
    /// `true` when this host ran `systemctl` for the updater units.
    pub systemd_probed: bool,
    /// `LastTriggerUSec` from `cortex-update.timer` (human-readable) when available.
    pub last_check: Option<String>,
    /// mtime of `/opt/rudy/current.sha` (or `RUDY_CURRENT_SHA_FILE`) in RFC 3339.
    pub last_applied: Option<String>,
    /// `is-active` for `cortex-update.timer` — `None` if not probed.
    pub timer_active: Option<bool>,
    /// `is-failed` for `cortex-update.service` — `None` if not probed.
    pub service_failed: Option<bool>,
    /// `true` when: not probed (dev) or (timer active and service not failed on Linux).
    pub healthy: bool,
}

/// Build stamp + `latest.json` + updater: surfaced on `ServerConfig` for the Connection card.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct DeploymentInfo {
    pub build: BuildIdentity,
    pub latest: ChannelLatest,
    /// `true` when we know `build.commit_sha != latest.commit_sha` (and both are present).
    pub is_stale: bool,
    /// `true` when `latest.json` was fetched and parsed; `is_stale` is meaningful when this is set.
    pub latest_manifest_ok: bool,
    pub updater: UpdaterStatus,
}

impl BuildIdentity {
    /// Values baked in at compile time (`build.rs` / `CORTEX_*` env in CI).
    pub fn from_build_env() -> Self {
        Self {
            commit_sha: env!("CORTEX_COMMIT_SHA").to_string(),
            short_sha: env!("CORTEX_SHORT_SHA").to_string(),
            built_at: env!("CORTEX_BUILT_AT").to_string(),
            package_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl UpdaterStatus {
    /// No `systemctl` (non-Linux or units missing) — not an error; `healthy` stays `true`.
    pub fn not_probed() -> Self {
        Self {
            systemd_probed: false,
            last_check: None,
            last_applied: None,
            timer_active: None,
            service_failed: None,
            healthy: true,
        }
    }
}

impl DeploymentInfo {
    pub fn initial() -> Self {
        let build = BuildIdentity::from_build_env();
        Self {
            build,
            latest: ChannelLatest {
                commit_sha: None,
                short_sha: None,
                built_at: None,
                manifest_error: None,
            },
            is_stale: false,
            latest_manifest_ok: false,
            updater: UpdaterStatus::not_probed(),
        }
    }
}

/// GET /api/config — what the UI needs to bootstrap.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ServerConfig {
    /// Workspace package version; same as `deployment.build.package_version`.
    pub version: String,
    /// RobStride models with a loaded `robstride_*.yaml` (e.g. `RS03`), sorted for stable JSON.
    pub actuator_models: Vec<String>,
    pub webtransport: WebTransportAdvert,
    pub features: ServerFeatures,
    pub paths: ServerPaths,
    pub deployment: DeploymentInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct WebTransportAdvert {
    pub enabled: bool,
    /// Fully-qualified URL the browser should open. Example:
    /// `https://rudy.your-tailnet.ts.net:4433/wt`.
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ServerFeatures {
    pub mock_can: bool,
    pub require_verified: bool,
}
