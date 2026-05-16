// Single source of truth for "how bad is this motor's latest feedback?"
// Used by Overview actuator tallies and Devices actuator cards so they
// cannot disagree while reading the same `queryKeys.motors.all()` cache.

import type { MotorSummary } from "@/lib/types/MotorSummary";
import { actionableFaultSta, hasRecoveryLatchOnly } from "@/lib/motorFaultDecode";

/** Same window as the dashboard actuator card row (motion preflight scale). */
export const MOTOR_TELEM_STALE_MS = 3_000;

export type MotorTelemetryTone =
  | "ok"
  | "advisory"
  | "warn"
  | "crit"
  | "stale"
  | "missing";

/**
 * Priority: missing telemetry → actionable fault → warning/advisory → stale age → ok.
 * `nowMs` is injectable for tests; production callers should omit it.
 */
export function motorTelemetryTone(
  m: MotorSummary,
  nowMs: number = Date.now(),
): MotorTelemetryTone {
  const fb = m.latest;
  if (!fb) return "missing";
  if (actionableFaultSta(fb.fault_sta) !== 0) return "crit";
  if (fb.warn_sta !== 0) return "warn";
  if (hasRecoveryLatchOnly(fb.fault_sta, fb.warn_sta)) return "advisory";
  if (nowMs - Number(fb.t_ms) > MOTOR_TELEM_STALE_MS) return "stale";
  return "ok";
}

export function motorTelemetryShortLabel(tone: MotorTelemetryTone): string {
  switch (tone) {
    case "missing":
      return "No data";
    case "stale":
      return "Stale";
    case "crit":
      return "Fault";
    case "advisory":
      return "Advisory";
    case "warn":
      return "Warn";
    case "ok":
      return "Live";
  }
}
