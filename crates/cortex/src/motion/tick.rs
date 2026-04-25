//! Controller tick / preflight horizon from [`crate::config::SafetyConfig`].

use crate::config::SafetyConfig;

/// RS03 active-report floor is 10 ms (`EPScan_time = 1`); stay at or above
/// that so MIT streaming does not outpace telemetry.
const MIN_TICK_MS: u64 = 10;

/// Upper clamp so a mis-set `mit_command_rate_hz` cannot stall watchdogs.
const MAX_TICK_MS: u64 = 200;

/// MIT / velocity motion tick interval (ms) from `mit_command_rate_hz`.
#[must_use]
pub fn motion_tick_interval_ms(safety: &SafetyConfig) -> u64 {
    let hz = safety.mit_command_rate_hz;
    if !hz.is_finite() || hz <= 0.0 {
        return MIN_TICK_MS;
    }
    let ms = (1000.0 / hz).round().max(1.0) as u64;
    ms.clamp(MIN_TICK_MS, MAX_TICK_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SafetyConfig;

    #[test]
    fn hundred_hz_is_ten_ms() {
        let s: SafetyConfig =
            serde_json::from_str(r#"{"mit_command_rate_hz":100.0}"#).expect("parse");
        assert_eq!(motion_tick_interval_ms(&s), 10);
    }

    #[test]
    fn low_hz_clamps_to_max_interval() {
        let s: SafetyConfig =
            serde_json::from_str(r#"{"mit_command_rate_hz":1.0}"#).expect("parse");
        assert_eq!(motion_tick_interval_ms(&s), MAX_TICK_MS);
    }

    #[test]
    fn nan_falls_back_to_min() {
        let mut s: SafetyConfig = serde_json::from_str("{}").expect("parse");
        s.mit_command_rate_hz = f32::NAN;
        assert_eq!(motion_tick_interval_ms(&s), MIN_TICK_MS);
    }
}
