//! Physical sensor workers.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::ImuSensorConfig;
use crate::state::SharedState;
#[cfg(target_os = "linux")]
use crate::types::ImuSample;
use crate::types::{SensorHealth, SensorSample};

#[cfg(target_os = "linux")]
mod bno085_i2c;

pub fn spawn(state: SharedState) {
    let cfg = state.cfg.sensors.imu.clone();
    if !cfg.enabled {
        tracing::info!("imu sensor disabled");
        return;
    }

    let state_for_worker = state.clone();
    std::thread::Builder::new()
        .name("cortex-bno085".to_string())
        .spawn(move || worker_loop(state_for_worker, cfg))
        .expect("spawn bno085 sensor worker");
}

pub fn latest(state: &SharedState) -> Vec<SensorSample> {
    let mut samples: BTreeMap<String, SensorSample> = state
        .latest_sensors
        .read()
        .expect("latest_sensors poisoned")
        .clone();

    let cfg = &state.cfg.sensors.imu;
    if cfg.enabled {
        samples
            .entry(cfg.id.clone())
            .or_insert_with(|| unavailable_sample(cfg, "sensor has not produced a sample yet"));
    }

    samples.into_values().map(mark_stale_if_needed).collect()
}

pub fn latest_one(state: &SharedState, sensor_id: &str) -> Option<SensorSample> {
    latest(state).into_iter().find(|s| s.sensor_id == sensor_id)
}

fn worker_loop(state: SharedState, cfg: ImuSensorConfig) {
    tracing::info!(
        sensor_id = %cfg.id,
        bus = cfg.bus,
        address = format_args!("0x{:02x}", cfg.address),
        poll_interval_ms = cfg.poll_interval_ms,
        "bno085 sensor worker spawned"
    );

    let retry_delay = Duration::from_secs(2);
    loop {
        match run_bno085(&state, &cfg) {
            Ok(()) => return,
            Err(e) => {
                tracing::warn!(
                    sensor_id = %cfg.id,
                    required = cfg.required,
                    error = %e,
                    "bno085 sensor worker failed; retrying"
                );
                publish(&state, error_sample(&cfg, e.to_string()));
                std::thread::sleep(retry_delay);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn run_bno085(state: &SharedState, cfg: &ImuSensorConfig) -> anyhow::Result<()> {
    let mut sensor = bno085_i2c::Bno085I2c::open(cfg.bus, cfg.address)?;
    sensor.initialize(Duration::from_secs(3), cfg.poll_interval_ms)?;

    let poll = Duration::from_millis(cfg.poll_interval_ms.max(5));
    loop {
        if let Some(imu) = sensor.read_latest()? {
            publish(state, ok_sample(cfg, imu));
        }
        std::thread::sleep(poll);
    }
}

#[cfg(not(target_os = "linux"))]
fn run_bno085(_state: &SharedState, _cfg: &ImuSensorConfig) -> anyhow::Result<()> {
    anyhow::bail!("BNO085 I2C support is only available on Linux")
}

fn publish(state: &SharedState, sample: SensorSample) {
    state
        .latest_sensors
        .write()
        .expect("latest_sensors poisoned")
        .insert(sample.sensor_id.clone(), sample.clone());
    let _ = state.sensor_tx.send(sample);
}

#[cfg(target_os = "linux")]
fn ok_sample(cfg: &ImuSensorConfig, imu: ImuSample) -> SensorSample {
    SensorSample {
        t_ms: now_ms(),
        sensor_id: cfg.id.clone(),
        frame_id: cfg.frame_id.clone(),
        kind: "imu".to_string(),
        health: SensorHealth::Ok,
        stale_after_ms: cfg.stale_after_ms,
        message: None,
        imu: Some(imu),
    }
}

fn unavailable_sample(cfg: &ImuSensorConfig, message: impl Into<String>) -> SensorSample {
    SensorSample {
        t_ms: now_ms(),
        sensor_id: cfg.id.clone(),
        frame_id: cfg.frame_id.clone(),
        kind: "imu".to_string(),
        health: SensorHealth::Unavailable,
        stale_after_ms: cfg.stale_after_ms,
        message: Some(message.into()),
        imu: None,
    }
}

fn error_sample(cfg: &ImuSensorConfig, message: impl Into<String>) -> SensorSample {
    SensorSample {
        health: SensorHealth::Error,
        ..unavailable_sample(cfg, message)
    }
}

fn mark_stale_if_needed(mut sample: SensorSample) -> SensorSample {
    if sample.health == SensorHealth::Ok {
        let age_ms = now_ms().saturating_sub(sample.t_ms) as u64;
        if age_ms > sample.stale_after_ms {
            sample.health = SensorHealth::Stale;
            sample.message = Some(format!("last sample is {age_ms} ms old"));
        }
    }
    sample
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
pub(crate) fn rotation_accuracy_label(status: u8) -> String {
    match status {
        0 => "unreliable",
        1 => "low",
        2 => "medium",
        3 => "high",
        _ => "unknown",
    }
    .to_string()
}
