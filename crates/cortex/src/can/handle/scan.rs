use std::io;
use std::time::Duration;

use anyhow::Result;

use crate::can::discovery::{
    BusParamReader, DeviceProbeRegistry, DiscoveredDevice, HardwareScanReport,
};
use crate::state::SharedState;

use super::LinuxCanCore;

impl BusParamReader for LinuxCanCore {
    fn read_type17_register(
        &self,
        iface: &str,
        motor_id: u8,
        index: u16,
        timeout: Duration,
    ) -> io::Result<Option<[u8; 4]>> {
        let handle = self
            .handle_for(iface)
            .map_err(|e| io::Error::other(format!("{e:#}")))?;
        handle.read_param(self.host_id, motor_id, index, timeout)
    }
}

/// Active scan: sequential `(bus × can_id)` probes via [`DeviceProbeRegistry`].
pub(crate) fn run_hardware_scan(
    core: &LinuxCanCore,
    state: &SharedState,
    bus_filter: Option<&str>,
    mut id_min: u8,
    mut id_max: u8,
    timeout: Duration,
) -> Result<HardwareScanReport> {
    if id_min > id_max {
        std::mem::swap(&mut id_min, &mut id_max);
    }
    id_min = id_min.max(1);
    id_max = id_max.min(0x7F);

    let buses: Vec<String> = if let Some(b) = bus_filter {
        if !core.cfg.can.buses.iter().any(|c| c.iface == b) {
            anyhow::bail!("bus {b:?} is not configured in [[can.buses]]");
        }
        vec![b.to_string()]
    } else {
        core.cfg.can.buses.iter().map(|c| c.iface.clone()).collect()
    };

    if buses.is_empty() {
        return Ok(HardwareScanReport {
            discovered: vec![],
            attempts: vec![],
            message: Some("no [[can.buses]] configured — nothing to scan".into()),
        });
    }

    let registry = DeviceProbeRegistry::with_default_probes();
    let mut discovered: Vec<DiscoveredDevice> = Vec::new();
    let mut attempts = Vec::new();

    for bus in buses {
        for can_id in id_min..=id_max {
            let (dev, mut row) = registry.probe_one_id(core, &bus, can_id, timeout);
            attempts.append(&mut row);
            if let Some(ref d) = dev {
                state.record_active_scan_seen(
                    &d.bus,
                    d.can_id,
                    d.family_hint.clone(),
                    d.identification_payload.clone(),
                );
                discovered.push(d.clone());
            }
        }
    }

    Ok(HardwareScanReport {
        discovered,
        attempts,
        message: None,
    })
}
