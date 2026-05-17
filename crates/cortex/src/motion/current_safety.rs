//! Current watchdog for active daemon-owned motion.
//!
//! This is host-side mitigation, not a certified safety function. It uses the
//! RobStride `iqf` type-17 observable as a live current signal, derives trip
//! bands from the actuator's effective `limit_cur`, and only stops a limb when
//! current remains high while cortex owns the planned path.

use std::collections::{BTreeMap, VecDeque};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::audit::{AuditEntry, AuditResult};
use crate::inventory::{Actuator, CurrentSafetyTiers};
use crate::state::SharedState;
use crate::types::{MotorFeedback, SafetyEvent};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentIncident {
    pub t_ms: i64,
    pub role: String,
    pub limb: String,
    pub tier: String,
    pub behavior: String,
    pub signed_current_arms: f32,
    pub abs_current_arms: f32,
    pub threshold_arms: f32,
    pub duration_ms: u64,
    pub i2t_value: f32,
    pub limit_cur_arms: f32,
    pub limit_torque_nm: Option<f32>,
    pub limit_source: String,
    pub motion_kind: String,
    pub motion_error_rad: f32,
    pub velocity_rad_s: f32,
}

#[derive(Debug, Clone)]
pub struct CurrentLatch {
    pub t_ms: i64,
    pub limb: String,
    pub role: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentBehavior {
    Log,
    Warn,
    LimbStopQuarantine,
}

#[derive(Debug, Clone)]
pub struct CurrentTrip {
    pub incident: CurrentIncident,
    pub behavior: CurrentBehavior,
}

pub struct WatchdogInput<'a> {
    pub state: &'a SharedState,
    pub motor: &'a Actuator,
    pub feedback: &'a MotorFeedback,
    pub motion_kind: &'a str,
    pub target_position_rad: Option<f32>,
    pub desired_vel_rad_s: f32,
    pub motion_elapsed_ms: u64,
    pub tick_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CurrentWatchState {
    pub over_moderate_since_ms: Option<i64>,
    pub over_severe_since_ms: Option<i64>,
    pub i2t_value: f32,
    pub last_update_ms: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct CurrentSafetyRuntime {
    pub per_role: BTreeMap<String, CurrentWatchState>,
    pub latches_by_limb: BTreeMap<String, CurrentLatch>,
    pub incidents: VecDeque<CurrentIncident>,
}

#[derive(Debug, Clone)]
struct EffectiveLimits {
    limit_cur_arms: f32,
    limit_torque_nm: Option<f32>,
    source: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct Thresholds {
    mild: f32,
    moderate: f32,
    severe: f32,
    i2t_ratio: f32,
}

pub fn effective_limb_id(motor: &Actuator) -> String {
    motor
        .common
        .limb
        .clone()
        .unwrap_or_else(|| motor.common.role.clone())
}

pub fn latch_for_role(state: &SharedState, role: &str) -> Option<CurrentLatch> {
    let motor = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role(role)
        .cloned()?;
    let limb = effective_limb_id(&motor);
    state
        .current_safety
        .read()
        .expect("current_safety poisoned")
        .latches_by_limb
        .get(&limb)
        .cloned()
}

pub fn clear_limb_latch(state: &SharedState, limb: &str) -> Option<CurrentLatch> {
    state
        .current_safety
        .write()
        .expect("current_safety poisoned")
        .latches_by_limb
        .remove(limb)
}

pub fn evaluate_active_path(input: WatchdogInput<'_>) -> Option<CurrentTrip> {
    let safety = input.state.read_effective().safety.clone();
    if !safety.current_watchdog_enabled {
        return None;
    }

    let signed_current = input.feedback.q_current_arms?;
    if !signed_current.is_finite() {
        return None;
    }

    let limits = resolve_effective_limits(input.state, input.motor)?;
    if limits.limit_cur_arms <= 0.0 || !limits.limit_cur_arms.is_finite() {
        return None;
    }

    let thresholds = derive_thresholds(input.motor.common.current_safety.as_ref(), &limits);
    let now_ms = Utc::now().timestamp_millis();
    let abs_current = signed_current.abs();
    let ratio = abs_current / limits.limit_cur_arms;
    let target = input.target_position_rad.unwrap_or(
        input.feedback.mech_pos_rad + input.desired_vel_rad_s * input.tick_ms as f32 / 1000.0,
    );
    let motion_error = (target - input.feedback.mech_pos_rad).abs();
    let stalled =
        input.desired_vel_rad_s.abs() > 0.01 && input.feedback.mech_vel_rad_s.abs() < 0.02;

    let mut runtime = input
        .state
        .current_safety
        .write()
        .expect("current_safety poisoned");
    let role = input.motor.common.role.clone();
    let (i2t_value, severe_duration) = {
        let ws = runtime.per_role.entry(role.clone()).or_default();
        let dt_s = ws
            .last_update_ms
            .map(|last| now_ms.saturating_sub(last).max(0) as f32 / 1000.0)
            .unwrap_or(input.tick_ms as f32 / 1000.0);
        ws.last_update_ms = Some(now_ms);

        let i2t_floor = thresholds.i2t_ratio.max(0.01);
        if ratio > i2t_floor {
            ws.i2t_value += (ratio * ratio) * dt_s;
        } else {
            ws.i2t_value = (ws.i2t_value - safety.current_i2t_decay_per_s * dt_s).max(0.0);
        }

        if input.motion_elapsed_ms >= safety.current_trip_min_motion_ms {
            if abs_current >= thresholds.moderate {
                ws.over_moderate_since_ms.get_or_insert(now_ms);
            } else {
                ws.over_moderate_since_ms = None;
            }
            if abs_current >= thresholds.severe {
                ws.over_severe_since_ms.get_or_insert(now_ms);
            } else {
                ws.over_severe_since_ms = None;
            }
        }
        let severe_duration = ws
            .over_severe_since_ms
            .map(|start| now_ms.saturating_sub(start).max(0) as u64)
            .unwrap_or(0);
        (ws.i2t_value, severe_duration)
    };

    if abs_current >= thresholds.mild {
        let incident = incident(
            &input,
            "mild",
            "log",
            signed_current,
            thresholds.mild,
            0,
            i2t_value,
            &limits,
            motion_error,
        );
        push_incident(
            &mut runtime,
            incident.clone(),
            safety.current_event_retention,
        );
        let _ = input
            .state
            .safety_event_tx
            .send(SafetyEvent::CurrentThresholdCrossed {
                t_ms: incident.t_ms,
                role: incident.role.clone(),
                limb: incident.limb.clone(),
                tier: incident.tier.clone(),
                signed_current_arms: incident.signed_current_arms,
                abs_current_arms: incident.abs_current_arms,
                threshold_arms: incident.threshold_arms,
                limit_cur_arms: incident.limit_cur_arms,
                limit_source: incident.limit_source.clone(),
                duration_ms: incident.duration_ms,
                observe_only: true,
            });
    }

    if input.motion_elapsed_ms < safety.current_trip_min_motion_ms {
        return None;
    }
    let i2t_trip = i2t_value >= safety.current_i2t_trip_budget;
    let severe_trip = severe_duration >= safety.current_trip_sustain_ms
        && abs_current >= thresholds.severe
        && (stalled || motion_error > 0.02);

    if !severe_trip && !i2t_trip {
        return None;
    }

    let tier = if i2t_trip { "i2t" } else { "severe" };
    let threshold = if i2t_trip {
        thresholds.i2t_ratio * limits.limit_cur_arms
    } else {
        thresholds.severe
    };
    let behavior = if safety.current_trip_observe_only {
        CurrentBehavior::Warn
    } else {
        CurrentBehavior::LimbStopQuarantine
    };
    let behavior_label = match behavior {
        CurrentBehavior::Log => "log",
        CurrentBehavior::Warn => "warn",
        CurrentBehavior::LimbStopQuarantine => "limb_stop_quarantine",
    };
    let incident = incident(
        &input,
        tier,
        behavior_label,
        signed_current,
        threshold,
        severe_duration,
        i2t_value,
        &limits,
        motion_error,
    );
    push_incident(
        &mut runtime,
        incident.clone(),
        safety.current_event_retention,
    );

    if matches!(behavior, CurrentBehavior::LimbStopQuarantine) {
        runtime.latches_by_limb.insert(
            incident.limb.clone(),
            CurrentLatch {
                t_ms: incident.t_ms,
                limb: incident.limb.clone(),
                role: incident.role.clone(),
                reason: format!(
                    "{} current {:.2} Arms >= {:.2} Arms for {} ms",
                    incident.tier,
                    incident.abs_current_arms,
                    incident.threshold_arms,
                    incident.duration_ms
                ),
            },
        );
    }

    drop(runtime);
    input.state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "current_safety_trip".into(),
        target: Some(incident.role.clone()),
        details: serde_json::to_value(&incident).unwrap_or(serde_json::Value::Null),
        result: if matches!(behavior, CurrentBehavior::LimbStopQuarantine) {
            AuditResult::Denied
        } else {
            AuditResult::Ok
        },
    });
    let _ = input.state.safety_event_tx.send(SafetyEvent::CurrentTrip {
        t_ms: incident.t_ms,
        role: incident.role.clone(),
        limb: incident.limb.clone(),
        tier: incident.tier.clone(),
        behavior: incident.behavior.clone(),
        signed_current_arms: incident.signed_current_arms,
        abs_current_arms: incident.abs_current_arms,
        threshold_arms: incident.threshold_arms,
        limit_cur_arms: incident.limit_cur_arms,
        limit_source: incident.limit_source.clone(),
        duration_ms: incident.duration_ms,
        i2t_value: incident.i2t_value,
        motion_kind: incident.motion_kind.clone(),
        observe_only: safety.current_trip_observe_only,
    });

    Some(CurrentTrip { incident, behavior })
}

pub fn evaluate_idle_sample(
    state: &SharedState,
    motor: &Actuator,
    feedback: &MotorFeedback,
) -> Option<CurrentTrip> {
    let safety = state.read_effective().safety.clone();
    if !safety.current_watchdog_enabled || state.motion.current(&motor.common.role).is_some() {
        return None;
    }
    let signed_current = feedback.q_current_arms?;
    let limits = resolve_effective_limits(state, motor)?;
    let tiers = motor.common.current_safety.as_ref()?;
    let severe_idle = tiers.severe_idle_ratio?;
    let threshold = limits.limit_cur_arms * severe_idle.clamp(0.0, 4.0);
    if threshold <= 0.0 || signed_current.abs() < threshold {
        return None;
    }

    let now_ms = Utc::now().timestamp_millis();
    let role = motor.common.role.clone();
    let mut runtime = state
        .current_safety
        .write()
        .expect("current_safety poisoned");
    let ws = runtime.per_role.entry(role.clone()).or_default();
    let started = ws.over_severe_since_ms.get_or_insert(now_ms);
    let duration_ms = now_ms.saturating_sub(*started).max(0) as u64;
    if duration_ms < safety.current_trip_sustain_ms {
        return None;
    }

    let behavior = if safety.current_trip_observe_only {
        CurrentBehavior::Warn
    } else {
        CurrentBehavior::LimbStopQuarantine
    };
    let behavior_label = match behavior {
        CurrentBehavior::Log => "log",
        CurrentBehavior::Warn => "warn",
        CurrentBehavior::LimbStopQuarantine => "limb_stop_quarantine",
    };
    let incident = CurrentIncident {
        t_ms: now_ms,
        role: role.clone(),
        limb: effective_limb_id(motor),
        tier: "severe_idle".into(),
        behavior: behavior_label.into(),
        signed_current_arms: signed_current,
        abs_current_arms: signed_current.abs(),
        threshold_arms: threshold,
        duration_ms,
        i2t_value: ws.i2t_value,
        limit_cur_arms: limits.limit_cur_arms,
        limit_torque_nm: limits.limit_torque_nm,
        limit_source: limits.source.into(),
        motion_kind: "idle".into(),
        motion_error_rad: 0.0,
        velocity_rad_s: feedback.mech_vel_rad_s,
    };
    push_incident(
        &mut runtime,
        incident.clone(),
        safety.current_event_retention,
    );
    if matches!(behavior, CurrentBehavior::LimbStopQuarantine) {
        runtime.latches_by_limb.insert(
            incident.limb.clone(),
            CurrentLatch {
                t_ms: incident.t_ms,
                limb: incident.limb.clone(),
                role: incident.role.clone(),
                reason: format!(
                    "severe idle current {:.2} Arms >= {:.2} Arms for {} ms",
                    incident.abs_current_arms, incident.threshold_arms, incident.duration_ms
                ),
            },
        );
    }
    drop(runtime);

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "current_safety_idle_trip".into(),
        target: Some(incident.role.clone()),
        details: serde_json::to_value(&incident).unwrap_or(serde_json::Value::Null),
        result: if matches!(behavior, CurrentBehavior::LimbStopQuarantine) {
            AuditResult::Denied
        } else {
            AuditResult::Ok
        },
    });
    let _ = state.safety_event_tx.send(SafetyEvent::CurrentTrip {
        t_ms: incident.t_ms,
        role: incident.role.clone(),
        limb: incident.limb.clone(),
        tier: incident.tier.clone(),
        behavior: incident.behavior.clone(),
        signed_current_arms: incident.signed_current_arms,
        abs_current_arms: incident.abs_current_arms,
        threshold_arms: incident.threshold_arms,
        limit_cur_arms: incident.limit_cur_arms,
        limit_source: incident.limit_source.clone(),
        duration_ms: incident.duration_ms,
        i2t_value: incident.i2t_value,
        motion_kind: incident.motion_kind.clone(),
        observe_only: safety.current_trip_observe_only,
    });
    Some(CurrentTrip { incident, behavior })
}

pub fn confirm_post_stop(state: &SharedState, incident: &CurrentIncident) {
    let safety = state.read_effective().safety.clone();
    let safe = incident.limit_cur_arms * safety.current_post_stop_safe_ratio;
    let latest = state
        .latest
        .read()
        .expect("latest poisoned")
        .get(&incident.role)
        .cloned();
    let current = latest.and_then(|fb| fb.q_current_arms).unwrap_or(0.0).abs();
    if current <= safe {
        return;
    }
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "current_safety_stop_escalated".into(),
        target: Some(incident.role.clone()),
        details: serde_json::json!({
            "role": incident.role,
            "limb": incident.limb,
            "post_stop_current_arms": current,
            "safe_current_arms": safe,
            "confirm_window_ms": safety.current_post_stop_confirm_ms,
        }),
        result: AuditResult::Denied,
    });
    let _ = state
        .safety_event_tx
        .send(SafetyEvent::CurrentStopEscalated {
            t_ms: Utc::now().timestamp_millis(),
            role: incident.role.clone(),
            limb: incident.limb.clone(),
            post_stop_current_arms: current,
            safe_current_arms: safe,
        });
}

fn push_incident(runtime: &mut CurrentSafetyRuntime, incident: CurrentIncident, retention: u32) {
    runtime.incidents.push_back(incident);
    let max = retention.max(1) as usize;
    while runtime.incidents.len() > max {
        runtime.incidents.pop_front();
    }
}

fn incident(
    input: &WatchdogInput<'_>,
    tier: &str,
    behavior: &str,
    signed_current: f32,
    threshold: f32,
    duration_ms: u64,
    i2t_value: f32,
    limits: &EffectiveLimits,
    motion_error_rad: f32,
) -> CurrentIncident {
    CurrentIncident {
        t_ms: Utc::now().timestamp_millis(),
        role: input.motor.common.role.clone(),
        limb: effective_limb_id(input.motor),
        tier: tier.to_string(),
        behavior: behavior.to_string(),
        signed_current_arms: signed_current,
        abs_current_arms: signed_current.abs(),
        threshold_arms: threshold,
        duration_ms,
        i2t_value,
        limit_cur_arms: limits.limit_cur_arms,
        limit_torque_nm: limits.limit_torque_nm,
        limit_source: limits.source.to_string(),
        motion_kind: input.motion_kind.to_string(),
        motion_error_rad,
        velocity_rad_s: input.feedback.mech_vel_rad_s,
    }
}

fn derive_thresholds(tiers: Option<&CurrentSafetyTiers>, limits: &EffectiveLimits) -> Thresholds {
    let default = CurrentSafetyTiers {
        mild_ratio: 0.60,
        moderate_ratio: 0.80,
        severe_ratio: 0.95,
        severe_idle_ratio: None,
        i2t_ratio: 0.75,
        severe_abs_arms: None,
    };
    let t = tiers.unwrap_or(&default);
    let clamp = |ratio: f32| (limits.limit_cur_arms * ratio.clamp(0.0, 4.0)).max(0.0);
    let mut severe = clamp(t.severe_ratio);
    if let Some(abs) = t.severe_abs_arms.filter(|v| v.is_finite() && *v > 0.0) {
        severe = severe.min(abs);
    }
    Thresholds {
        mild: clamp(t.mild_ratio),
        moderate: clamp(t.moderate_ratio),
        severe,
        i2t_ratio: t.i2t_ratio.clamp(0.01, 4.0),
    }
}

fn resolve_effective_limits(state: &SharedState, motor: &Actuator) -> Option<EffectiveLimits> {
    if let Some(v) = live_param_f32(state, &motor.common.role, "limit_cur") {
        return Some(EffectiveLimits {
            limit_cur_arms: v,
            limit_torque_nm: live_param_f32(state, &motor.common.role, "limit_torque")
                .or_else(|| desired_param_f32(motor, "limit_torque")),
            source: "readback_confirmed",
        });
    }
    if let Some(v) = desired_param_f32(motor, "limit_cur") {
        return Some(EffectiveLimits {
            limit_cur_arms: v,
            limit_torque_nm: desired_param_f32(motor, "limit_torque"),
            source: "desired_only",
        });
    }
    let spec = state.spec_for(motor.robstride_model());
    let fallback = spec
        .commissioning_defaults
        .get("limit_cur_a")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .or_else(|| {
            spec.firmware_limits
                .get("limit_cur")
                .and_then(|d| d.hardware_range.map(|r| r[1]))
        })
        .or_else(|| {
            (spec.hardware.phase_current_rated_apk > 0.0)
                .then_some(spec.hardware.phase_current_rated_apk)
        })?;
    Some(EffectiveLimits {
        limit_cur_arms: fallback,
        limit_torque_nm: spec
            .commissioning_defaults
            .get("limit_torque_nm")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32),
        source: "fallback_default",
    })
}

fn live_param_f32(state: &SharedState, role: &str, name: &str) -> Option<f32> {
    state
        .params
        .read()
        .expect("params poisoned")
        .get(role)?
        .values
        .get(name)?
        .value
        .as_f64()
        .map(|v| v as f32)
}

fn desired_param_f32(motor: &Actuator, name: &str) -> Option<f32> {
    motor
        .common
        .desired_params
        .get(name)?
        .as_f64()
        .map(|v| v as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_thresholds_clamp_to_absolute_severe() {
        let limits = EffectiveLimits {
            limit_cur_arms: 10.0,
            limit_torque_nm: None,
            source: "test",
        };
        let tiers = CurrentSafetyTiers {
            mild_ratio: 0.5,
            moderate_ratio: 0.8,
            severe_ratio: 1.2,
            severe_idle_ratio: None,
            i2t_ratio: 0.7,
            severe_abs_arms: Some(9.0),
        };
        let t = derive_thresholds(Some(&tiers), &limits);
        assert_eq!(t.mild, 5.0);
        assert_eq!(t.moderate, 8.0);
        assert_eq!(t.severe, 9.0);
    }
}
