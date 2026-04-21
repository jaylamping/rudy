/** Angular conversion helpers for the SPA (wire format stays in radians). */

export const RAD_TO_DEG = 180 / Math.PI;
export const DEG_TO_RAD = Math.PI / 180;

export function radToDeg(rad: number): number {
  return rad * RAD_TO_DEG;
}

export function degToRad(deg: number): number {
  return deg * DEG_TO_RAD;
}

/** Formats a position in radians as degrees with a degree symbol, or "-" if not finite. */
export function formatAngleDeg(
  rad: number | null | undefined,
  fractionDigits = 2,
): string {
  if (rad == null || !Number.isFinite(rad)) return "-";
  return `${radToDeg(rad).toFixed(fractionDigits)}°`;
}

/** Formats angular velocity (rad/s) as deg/s with unit suffix. */
export function formatAngularVelDeg(
  radPerSec: number | null | undefined,
  fractionDigits = 1,
): string {
  if (radPerSec == null || !Number.isFinite(radPerSec)) return "-";
  const sign = radPerSec >= 0 ? "" : "-";
  const mag = Math.abs(radToDeg(radPerSec));
  return `${sign}${mag.toFixed(fractionDigits)}°/s`;
}

function normUnit(u: string | null | undefined): string {
  return (u ?? "").trim().toLowerCase();
}

export function isAngleUnit(u: string | null | undefined): boolean {
  const n = normUnit(u);
  return n === "rad" || n === "radian" || n === "radians";
}

export function isAngularVelUnit(u: string | null | undefined): boolean {
  const n = normUnit(u);
  return (
    n === "rad_per_s" ||
    n === "rad/s" ||
    n === "radians_per_second" ||
    n === "radians/s"
  );
}
