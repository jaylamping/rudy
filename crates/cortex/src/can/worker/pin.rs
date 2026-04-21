//! CPU affinity for per-bus SocketCAN workers.

use core_affinity;
use tracing::{debug, info};

/// Pin the current thread to `cpu` when possible.
pub(super) fn pin_to_cpu(iface: &str, cpu: usize) {
    let cores = match core_affinity::get_core_ids() {
        Some(c) => c,
        None => {
            debug!(
                iface = %iface,
                requested = cpu,
                "core_affinity unavailable; bus worker is unpinned"
            );
            return;
        }
    };
    let Some(core) = cores.get(cpu).copied() else {
        debug!(
            iface = %iface,
            requested = cpu,
            available = cores.len(),
            "requested CPU id out of range; bus worker is unpinned"
        );
        return;
    };
    if core_affinity::set_for_current(core) {
        info!(iface = %iface, cpu = cpu, "bus worker pinned to CPU");
    } else {
        debug!(
            iface = %iface,
            cpu = cpu,
            "set_for_current returned false; bus worker is unpinned"
        );
    }
}

/// Auto-assignment helper: given the inventory's [[can.buses]] order and
/// the available CPU count, pick the per-bus CPU id for `index`.
///
/// Policy: leave core 0 to the kernel + tokio runtime; spread bus
/// workers round-robin starting at core 1. Returns `None` when the
/// system has fewer than 2 cores (single-core means everything shares
/// core 0; pinning is pointless).
pub fn auto_assign_cpu(index: usize, cpu_count: usize) -> Option<usize> {
    if cpu_count < 2 {
        return None;
    }
    let n = cpu_count.saturating_sub(1);
    Some(1 + (index % n))
}

/// Logical CPU count for worker placement (`start_workers`).
pub fn available_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
