//! Per-bus counters for observability (decode failures, RX cadence, …).

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct BusHealth {
    pub frames_rx: AtomicU64,
    pub type2_decode_failures: AtomicU64,
    pub fault_frames_rx: AtomicU64,
    pub commands_drained: AtomicU64,
}

impl BusHealth {
    #[inline]
    pub fn record_rx_frame(&self) {
        self.frames_rx.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_type2_decode_fail(&self) {
        self.type2_decode_failures.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_fault_frame(&self) {
        self.fault_frames_rx.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_cmd_drained(&self, n: u64) {
        self.commands_drained.fetch_add(n, Ordering::Relaxed);
    }
}
