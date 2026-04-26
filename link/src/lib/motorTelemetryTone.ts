// Single source of truth for "how bad is this motor's latest feedback?"
// Used by Overview actuator tallies and Devices actuator cards so they
// cannot disagree while reading the same `queryKeys.motors.all()` cache.

import type { MotorSummary } from "@/lib/types/MotorSummary";

/** Same window as the dashboard actuator card row (motion preflight scale). */
export const MOTOR_TELEM_STALE_MS = 3_000;

export type MotorTelemetryTone = "ok" | "warn" | "crit" | "stale" | "missing";

/**
 * Priority: missing telemetry → fault → warning → stale age → ok.
 * `nowMs` is injectable for tests; production callers should omit it.
 */
export function motorTelemetryTone(
  m: MotorSummary,
  nowMs: number = Date.now(),
): MotorTelemetryTone {
  const fb = m.latest;
  if (!fb) return "missing";
  if (fb.fault_sta !== 0) return "crit";
  if (fb.warn_sta !== 0) return "warn";
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
    case "warn":
      return "Warn";
    case "ok":
      return "Live";
  }
}
