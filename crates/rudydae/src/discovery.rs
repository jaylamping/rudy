//! Active device discovery: probe registry and RobStride-oriented probes.
//!
//! Passive CAN IDs are tracked in [`crate::state::AppState::seen_can_ids`]; this
//! module drives **active** type-17 probes used by `POST /api/hardware/scan`.

use std::io;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// One device that responded to a probe during a scan run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredDevice {
    pub bus: String,
    pub can_id: u8,
    pub family_hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identification_payload: Option<serde_json::Value>,
}

/// One probe attempt at a single `(bus, can_id)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanAttempt {
    pub bus: String,
    pub can_id: u8,
    pub probe: String,
    pub found: bool,
}

/// Result bundle for `can::hardware_active_scan`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareScanReport {
    pub discovered: Vec<DiscoveredDevice>,
    pub attempts: Vec<ScanAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl HardwareScanReport {
    pub fn empty(message: impl Into<String>) -> Self {
        Self {
            discovered: vec![],
            attempts: vec![],
            message: Some(message.into()),
        }
    }
}

/// Minimal read primitive for probes (implemented by [`crate::can::LinuxCanCore`]).
pub trait BusParamReader: Send + Sync {
    fn read_type17_register(
        &self,
        iface: &str,
        motor_id: u8,
        index: u16,
        timeout: Duration,
    ) -> io::Result<Option<[u8; 4]>>;
}

/// A family-specific active probe. Probes run in registration order; the first
/// `Some(_)` wins for that `(bus, can_id)`.
pub trait DeviceProbe: Send + Sync {
    fn family_name(&self) -> &'static str;

    fn probe(
        &self,
        reader: &dyn BusParamReader,
        bus: &str,
        can_id: u8,
        timeout: Duration,
    ) -> Option<DiscoveredDevice>;
}

/// RobStride discovery: type-17 read of `firmware_version` (0x1003 per RS03 spec).
/// Any `Ok(_)` reply (including firmware read-fail status) counts as presence.
#[derive(Debug, Clone, Copy)]
pub struct RobstrideProbe {
    pub firmware_version_index: u16,
}

impl Default for RobstrideProbe {
    fn default() -> Self {
        Self {
            firmware_version_index: 0x1003,
        }
    }
}

impl DeviceProbe for RobstrideProbe {
    fn family_name(&self) -> &'static str {
        "robstride"
    }

    fn probe(
        &self,
        reader: &dyn BusParamReader,
        bus: &str,
        can_id: u8,
        timeout: Duration,
    ) -> Option<DiscoveredDevice> {
        let reply = reader
            .read_type17_register(bus, can_id, self.firmware_version_index, timeout)
            .ok()?;
        let identification_payload = match reply {
            Some(bytes) => {
                let snippet = String::from_utf8_lossy(&bytes)
                    .trim_end_matches('\0')
                    .trim()
                    .to_string();
                Some(serde_json::json!({
                    "param_index": format!("0x{:04x}", self.firmware_version_index),
                    "firmware_version_snippet": snippet,
                }))
            }
            None => Some(serde_json::json!({
                "param_index": format!("0x{:04x}", self.firmware_version_index),
                "read_status": "fail",
            })),
        };
        Some(DiscoveredDevice {
            bus: bus.to_string(),
            can_id,
            family_hint: self.family_name().to_string(),
            identification_payload,
        })
    }
}

/// Ordered list of probes used during a scan.
pub struct DeviceProbeRegistry {
    probes: Vec<Box<dyn DeviceProbe>>,
}

impl DeviceProbeRegistry {
    pub fn with_default_probes() -> Self {
        Self {
            probes: vec![Box::new(RobstrideProbe::default())],
        }
    }

    /// Returns `(discovered, attempts)` for this `(bus, can_id)`. Stops at the first
    /// probe that returns `Some`.
    pub fn probe_one_id(
        &self,
        reader: &dyn BusParamReader,
        bus: &str,
        can_id: u8,
        timeout: Duration,
    ) -> (Option<DiscoveredDevice>, Vec<ScanAttempt>) {
        let mut attempts = Vec::new();
        for p in &self.probes {
            let probe_name = p.family_name().to_string();
            if let Some(dev) = p.probe(reader, bus, can_id, timeout) {
                attempts.push(ScanAttempt {
                    bus: bus.to_string(),
                    can_id,
                    probe: probe_name,
                    found: true,
                });
                return (Some(dev), attempts);
            }
            attempts.push(ScanAttempt {
                bus: bus.to_string(),
                can_id,
                probe: probe_name,
                found: false,
            });
        }
        (None, attempts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockReader {
        ok_reply: Option<Option<[u8; 4]>>,
    }

    impl BusParamReader for MockReader {
        fn read_type17_register(
            &self,
            _iface: &str,
            _motor_id: u8,
            _index: u16,
            _timeout: Duration,
        ) -> io::Result<Option<[u8; 4]>> {
            self.ok_reply
                .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "mock timeout"))
        }
    }

    #[test]
    fn robstride_probe_none_reply_counts_as_present() {
        let reader = MockReader {
            ok_reply: Some(None),
        };
        let p = RobstrideProbe::default();
        let dev = p
            .probe(&reader, "can0", 0x55, Duration::from_millis(1))
            .unwrap();
        assert_eq!(dev.bus, "can0");
        assert_eq!(dev.can_id, 0x55);
        assert_eq!(dev.family_hint, "robstride");
        assert!(dev.identification_payload.is_some());
    }

    #[test]
    fn robstride_probe_timeout_yields_none() {
        let reader = MockReader { ok_reply: None };
        let p = RobstrideProbe::default();
        assert!(p
            .probe(&reader, "can0", 0x55, Duration::from_millis(1))
            .is_none());
    }

    #[test]
    fn registry_first_probe_wins() {
        let reader = MockReader {
            ok_reply: Some(Some(*b"v1\0\0")),
        };
        let reg = DeviceProbeRegistry::with_default_probes();
        let (dev, att) = reg.probe_one_id(&reader, "can0", 0x10, Duration::from_millis(1));
        assert!(dev.is_some());
        assert_eq!(att.len(), 1);
        assert!(att[0].found);
    }
}
