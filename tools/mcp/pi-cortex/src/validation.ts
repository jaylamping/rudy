const MAX_LOG_LINES = 2000;
const DEFAULT_LOG_LINES = 200;

export function clampInt(n: unknown, def: number, min: number, max: number): number {
  if (typeof n !== "number" || !Number.isFinite(n)) return def;
  return Math.min(max, Math.max(min, Math.trunc(n)));
}

export function logLines(n: unknown): number {
  return clampInt(n, DEFAULT_LOG_LINES, 1, MAX_LOG_LINES);
}

export function healthWaitMs(n: unknown): number {
  return clampInt(n, 30_000, 1000, 120_000);
}

export function healthPollMs(n: unknown): number {
  return clampInt(n, 1000, 200, 10_000);
}

/** Optional `journalctl --since=`; reject weird chars to limit injection. */
export function sanitizeSince(s: unknown): string | undefined {
  if (typeof s !== "string") return undefined;
  const t = s.trim();
  if (!t || t.length > 64) return undefined;
  if (!/^[0-9a-zA-Z.: \t_+-]+$/.test(t)) return undefined;
  return t;
}

/** Max seconds for health wait loops on Pi (maps from ms args). */
export function healthMaxWaitSec(waitMs: number): number {
  const sec = Math.ceil(waitMs / 1000);
  return clampInt(sec, 30, 5, 300);
}
