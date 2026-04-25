//! Runtime settings: `GET/PUT /api/settings`, reset, reseed, recovery, profiles.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::Utc;
use serde_json::{json, Value as JsonValue};

use crate::audit::{AuditEntry, AuditResult};
use crate::settings::data;
use crate::settings::registry::{self, WireApplyMode};
use crate::settings::{
    apply_key_from_json, apply_recovery, file_defaults_to_kv, merge_from_kv, validate_snapshot,
};
use crate::state::EffectiveRuntime;
use crate::state::SharedState;
use crate::types::{
    ApiError, PutSettingRequest, PutSettingResponse, SettingEntry, SettingsApplyMode,
    SettingsGetResponse, SettingsProfileApplyResponse, SettingsProfileCreateRequest,
    SettingsProfileCreateResponse, SettingsProfileInfo, SettingsProfilesListResponse,
    SettingsRecoveryAckResponse, SettingsResetResponse,
};
use crate::util::session_from_headers;

use crate::api::error;
use crate::api::lock_gate;

const RESEED_CONFIRM_HEADER: &str = "X-Rudy-Reseed-Confirm";

fn err(code: StatusCode, e: &str, detail: Option<String>) -> (StatusCode, Json<ApiError>) {
    error::err(code, e, detail)
}

fn map_apply(def: &registry::SettingDef) -> SettingsApplyMode {
    match def.apply_mode {
        WireApplyMode::ReadOnly => SettingsApplyMode::ReadOnly,
        WireApplyMode::RuntimeImmediate => SettingsApplyMode::RuntimeImmediate,
        WireApplyMode::RequiresRestart => SettingsApplyMode::RequiresRestart,
    }
}

/// `GET /api/settings`
pub async fn get_all(
    State(state): State<SharedState>,
) -> Result<Json<SettingsGetResponse>, (StatusCode, Json<ApiError>)> {
    let recovery_pending = state.settings_recovery_pending.load(Ordering::SeqCst);
    let runtime = state.settings_db.is_some();

    let db_kv: BTreeMap<String, String> = if let Some(db) = &state.settings_db {
        let d = db
            .lock()
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "settings_db_lock", None))?;
        data::list_kv(&d)
            .map_err(|e: anyhow::Error| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "settings_list_kv",
                    Some(e.to_string()),
                )
            })?
            .into_iter()
            .map(|(k, j, _)| (k, j))
            .collect()
    } else {
        BTreeMap::new()
    };

    let (s, t) = {
        let e = state.read_effective();
        (e.safety.clone(), e.telemetry.clone())
    };

    let mut entries = Vec::new();
    for def in registry::ALL {
        let seed = registry::value_from_file_cfg(&state.cfg, def.key).ok_or_else(|| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings_seed_missing",
                Some(def.key.to_string()),
            )
        })?;
        let eff = registry::value_from_merged(&s, &t, def.key).ok_or_else(|| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings_effective_missing",
                Some(def.key.to_string()),
            )
        })?;
        let in_db = if runtime {
            db_kv.contains_key(def.key)
        } else {
            false
        };
        let dirty = if runtime {
            registry::is_dirty_merged(&s, &t, &state.cfg, def.key, in_db)
        } else {
            false
        };

        let (editable, read_only_reason) = if !runtime {
            (
                false,
                Some("runtime store disabled: enable [runtime] in cortex.toml".into()),
            )
        } else {
            (def.tunable, None)
        };

        let apply_mode = if !runtime || !def.tunable {
            SettingsApplyMode::ReadOnly
        } else {
            map_apply(def)
        };

        entries.push(SettingEntry {
            key: def.key.to_string(),
            label: def.label.to_string(),
            description: def.description.to_string(),
            category: def.category.to_string(),
            value_kind: def.value_kind.to_string(),
            unit: def.unit.map(str::to_string),
            min: def.min,
            max: def.max,
            seed: seed.clone(),
            effective: eff.clone(),
            in_db,
            dirty,
            apply_mode,
            editable,
            read_only_reason,
            requires_motors_stopped: def.requires_motors_stopped,
        });
    }

    Ok(Json(SettingsGetResponse {
        runtime_db_enabled: runtime,
        recovery_pending,
        entries,
    }))
}

fn persist_effective(
    state: &SharedState,
    s: &crate::config::SafetyConfig,
    t: &crate::config::TelemetryConfig,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let db = state
        .settings_db
        .as_ref()
        .ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "runtime_db_disabled", None))?;
    let vec = file_defaults_to_kv(&state.cfg);
    let mut m: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in vec {
        let s_val = v.to_string();
        m.insert(k, s_val);
    }
    // Overlay currently merged values (full snapshot).
    for def in registry::ALL {
        if let Some(v) = registry::value_from_merged(s, t, def.key) {
            m.insert(def.key.to_string(), v.to_string());
        }
    }
    let rows: Vec<(String, JsonValue)> = m
        .into_iter()
        .map(|(k, s)| {
            let j: JsonValue = serde_json::from_str(&s).map_err(|e| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "settings_json",
                    Some(format!("{k}: {e}")),
                )
            })?;
            Ok((k, j))
        })
        .collect::<Result<_, _>>()?;
    let mut d = db
        .lock()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "settings_db_lock", None))?;
    data::replace_all_kv(&mut d, &rows).map_err(|e: anyhow::Error| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "settings_persist",
            Some(e.to_string()),
        )
    })
}

/// `PUT /api/settings/*key` — one key, validated, persisted, swapped.
pub async fn put_one(
    State(state): State<SharedState>,
    Path(key): Path<String>,
    headers: HeaderMap,
    Json(req): Json<PutSettingRequest>,
) -> Result<Json<PutSettingResponse>, (StatusCode, Json<ApiError>)> {
    if state.settings_db.is_none() {
        return Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_db_disabled",
            None,
        ));
    }
    let def = registry::def_by_key(&key)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_setting", Some(key.clone())))?;
    if !def.tunable {
        return Err(err(StatusCode::BAD_REQUEST, "read_only", Some(key.clone())));
    }
    lock_gate::require_control(&state, &headers)?;

    if def.requires_motors_stopped && !state.enabled.read().expect("enabled poisoned").is_empty() {
        return Err(err(
            StatusCode::CONFLICT,
            "motors_not_stopped",
            Some("stop all motors before changing this setting".into()),
        ));
    }

    if key == "safety.auto_home_on_boot" {
        let want = req
            .value
            .as_bool()
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_value", None))?;
        if want && state.settings_recovery_pending.load(Ordering::SeqCst) {
            return Err(err(
                StatusCode::CONFLICT,
                "recovery_pending",
                Some("acknowledge recovery (POST /api/settings/recovery/ack) before re-enabling auto home".into()),
            ));
        }
    }

    let (mut s, mut t) = {
        let e = state.read_effective();
        (e.safety.clone(), e.telemetry.clone())
    };
    let value_for_audit = req.value.clone();
    apply_key_from_json(&mut s, &mut t, &key, req.value)
        .map_err(|e| err(StatusCode::BAD_REQUEST, "invalid_value", Some(e)))?;
    apply_recovery(
        &mut s,
        state.settings_recovery_pending.load(Ordering::SeqCst),
    );
    validate_snapshot(&s, &t)
        .map_err(|e| err(StatusCode::BAD_REQUEST, "validation_failed", Some(e)))?;

    let eff = registry::value_from_merged(&s, &t, &key).ok_or_else(|| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "effective_readback",
            None,
        )
    })?;

    persist_effective(&state, &s, &t)?;
    state.set_effective(EffectiveRuntime {
        safety: s,
        telemetry: t,
    });

    let session = session_from_headers(&headers);
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "settings_put".into(),
        target: Some(key.clone()),
        details: json!({ "key": &key, "value": &value_for_audit }),
        result: AuditResult::Ok,
    });

    let apply_mode = map_apply(def);
    let note = if def.apply_mode == WireApplyMode::RequiresRestart {
        Some("telemetry loop may need daemon restart to pick up poll interval".into())
    } else {
        None
    };
    Ok(Json(PutSettingResponse {
        ok: true,
        key,
        effective: eff,
        apply_mode,
        note,
    }))
}

/// `POST /api/settings/reset` — re-import seed from TOML (full replace of KV + merge).
pub async fn post_reset(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<Json<SettingsResetResponse>, (StatusCode, Json<ApiError>)> {
    if state.settings_db.is_none() {
        return Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_db_disabled",
            None,
        ));
    }
    lock_gate::require_control(&state, &headers)?;
    if !state.enabled.read().expect("enabled poisoned").is_empty() {
        return Err(err(StatusCode::CONFLICT, "motors_not_stopped", None));
    }
    do_reset_reseed(&state, false)?;
    let session = session_from_headers(&headers);
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "settings_reset".into(),
        target: None,
        details: json!({}),
        result: AuditResult::Ok,
    });
    Ok(Json(SettingsResetResponse { ok: true }))
}

/// `POST /api/settings/reseed` — same as reset; requires `X-Rudy-Reseed-Confirm: 1` for audit trail.
pub async fn post_reseed(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<Json<SettingsResetResponse>, (StatusCode, Json<ApiError>)> {
    if state.settings_db.is_none() {
        return Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_db_disabled",
            None,
        ));
    }
    if headers
        .get(RESEED_CONFIRM_HEADER)
        .and_then(|h| h.to_str().ok())
        != Some("1")
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "reseed_not_confirmed",
            Some("set X-Rudy-Reseed-Confirm: 1".into()),
        ));
    }
    lock_gate::require_control(&state, &headers)?;
    if !state.enabled.read().expect("enabled poisoned").is_empty() {
        return Err(err(StatusCode::CONFLICT, "motors_not_stopped", None));
    }
    do_reset_reseed(&state, true)?;
    let session = session_from_headers(&headers);
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "settings_reseed".into(),
        target: None,
        details: json!({ "confirmed": true }),
        result: AuditResult::Ok,
    });
    Ok(Json(SettingsResetResponse { ok: true }))
}

fn do_reset_reseed(
    state: &SharedState,
    _from_reseed: bool,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let db = state
        .settings_db
        .as_ref()
        .ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "runtime_db_disabled", None))?;
    let mut d = db
        .lock()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "settings_db_lock", None))?;
    let rows: Vec<(String, JsonValue)> = file_defaults_to_kv(&state.cfg);
    data::replace_all_kv(&mut d, &rows).map_err(|e: anyhow::Error| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "settings_persist",
            Some(e.to_string()),
        )
    })?;
    let kv: BTreeMap<String, String> = data::list_kv(&d)
        .map_err(|e: anyhow::Error| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings_list_kv",
                Some(e.to_string()),
            )
        })?
        .into_iter()
        .map(|(k, j, _)| (k, j))
        .collect();
    drop(d);
    let (mut s, t) = merge_from_kv(&state.cfg, kv).map_err(|e: anyhow::Error| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "settings_merge",
            Some(e.to_string()),
        )
    })?;
    apply_recovery(
        &mut s,
        state.settings_recovery_pending.load(Ordering::SeqCst),
    );
    validate_snapshot(&s, &t)
        .map_err(|e| err(StatusCode::BAD_REQUEST, "validation_failed", Some(e)))?;
    state.set_effective(EffectiveRuntime {
        safety: s,
        telemetry: t,
    });
    Ok(())
}

/// `POST /api/settings/recovery/ack` — allow motion again (does not re-enable auto home).
pub async fn post_recovery_ack(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<Json<SettingsRecoveryAckResponse>, (StatusCode, Json<ApiError>)> {
    lock_gate::require_control(&state, &headers)?;
    state
        .settings_recovery_pending
        .store(false, Ordering::SeqCst);
    let session = session_from_headers(&headers);
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "settings_recovery_ack".into(),
        target: None,
        details: json!({}),
        result: AuditResult::Ok,
    });
    Ok(Json(SettingsRecoveryAckResponse {
        ok: true,
        recovery_pending: false,
    }))
}

/// `GET /api/settings/profiles` — `meta` rows `profile:*`.
pub async fn get_profiles(
    State(state): State<SharedState>,
) -> Result<Json<SettingsProfilesListResponse>, (StatusCode, Json<ApiError>)> {
    if state.settings_db.is_none() {
        return Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_db_disabled",
            None,
        ));
    }
    let db = state.settings_db.as_ref().unwrap();
    let d = db
        .lock()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "settings_db_lock", None))?;
    let rows = data::list_meta_with_prefix(&d, "profile:").map_err(|e: anyhow::Error| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "list_profiles",
            Some(e.to_string()),
        )
    })?;
    drop(d);
    let mut profiles = Vec::new();
    for (k, v) in rows {
        let name = registry::profile_name_from_meta_key(&k)
            .unwrap_or(&k)
            .to_string();
        let n_keys = serde_json::from_str::<BTreeMap<String, JsonValue>>(&v)
            .map(|m| m.len())
            .unwrap_or(0);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        v.hash(&mut h);
        let fingerprint = format!("{:016x}", h.finish());
        profiles.push(SettingsProfileInfo {
            name,
            key_count: n_keys,
            fingerprint,
        });
    }
    Ok(Json(SettingsProfilesListResponse { profiles }))
}

/// `POST /api/settings/profiles` — store a profile snapshot in `meta`.
pub async fn post_create_profile(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<SettingsProfileCreateRequest>,
) -> Result<Json<SettingsProfileCreateResponse>, (StatusCode, Json<ApiError>)> {
    if state.settings_db.is_none() {
        return Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_db_disabled",
            None,
        ));
    }
    lock_gate::require_control(&state, &headers)?;
    let mkey = registry::profile_meta_key(&req.name).ok_or_else(|| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_profile_name",
            Some("use [a-zA-Z0-9_-] only".into()),
        )
    })?;
    for k in req.values.keys() {
        if registry::def_by_key(k).is_none() {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "unknown_key_in_profile",
                Some(k.clone()),
            ));
        }
    }
    let body = serde_json::to_string(&req.values).map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "json_encode",
            Some(e.to_string()),
        )
    })?;
    let db = state.settings_db.as_ref().unwrap();
    let d = db
        .lock()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "settings_db_lock", None))?;
    data::set_meta(&d, &mkey, &body).map_err(|e: anyhow::Error| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "meta_set",
            Some(e.to_string()),
        )
    })?;
    drop(d);
    let session = session_from_headers(&headers);
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "settings_profile_create".into(),
        target: Some(req.name.clone()),
        details: json!({ "keys": req.values.len() }),
        result: AuditResult::Ok,
    });
    Ok(Json(SettingsProfileCreateResponse {
        ok: true,
        name: req.name,
    }))
}

/// `POST /api/settings/profiles/*name/apply` — merge profile onto seed and persist full KV.
pub async fn post_apply_profile(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<Json<SettingsProfileApplyResponse>, (StatusCode, Json<ApiError>)> {
    if state.settings_db.is_none() {
        return Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_db_disabled",
            None,
        ));
    }
    lock_gate::require_control(&state, &headers)?;
    if !state.enabled.read().expect("enabled poisoned").is_empty() {
        return Err(err(StatusCode::CONFLICT, "motors_not_stopped", None));
    }
    let mkey = registry::profile_meta_key(&name)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_profile_name", None))?;
    let db = state.settings_db.as_ref().unwrap();
    let d = db
        .lock()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "settings_db_lock", None))?;
    let json_str = data::get_meta(&d, &mkey)
        .map_err(|e: anyhow::Error| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "meta_get",
                Some(e.to_string()),
            )
        })?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "profile_not_found",
                Some(name.clone()),
            )
        })?;
    drop(d);
    let map: BTreeMap<String, JsonValue> = serde_json::from_str(&json_str).map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "profile_json",
            Some(e.to_string()),
        )
    })?;

    let mut s = state.cfg.safety.clone();
    let mut t = state.cfg.telemetry.clone();
    for (k, v) in &map {
        apply_key_from_json(&mut s, &mut t, k, v.clone()).map_err(|e| {
            err(
                StatusCode::BAD_REQUEST,
                "profile_key",
                Some(format!("{k}: {e}")),
            )
        })?;
    }
    apply_recovery(
        &mut s,
        state.settings_recovery_pending.load(Ordering::SeqCst),
    );
    validate_snapshot(&s, &t)
        .map_err(|e| err(StatusCode::BAD_REQUEST, "validation_failed", Some(e)))?;

    persist_effective(&state, &s, &t)?;
    state.set_effective(EffectiveRuntime {
        safety: s,
        telemetry: t,
    });
    let session = session_from_headers(&headers);
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "settings_profile_apply".into(),
        target: Some(name.clone()),
        details: json!({ "keys": map.len() }),
        result: AuditResult::Ok,
    });
    Ok(Json(SettingsProfileApplyResponse {
        ok: true,
        name,
        note: None,
    }))
}
