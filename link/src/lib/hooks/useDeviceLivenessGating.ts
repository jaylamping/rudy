import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { queryKeys } from "@/api";
import { api } from "@/lib/api";
import { DEVICE_LIVE_STALE_MS } from "@/lib/deviceLiveness";
import { useLiveInterval } from "@/lib/hooks/useLiveInterval";
import type { MotorSummary } from "@/lib/types/MotorSummary";

function isMotorLiveInList(list: MotorSummary[] | undefined, role: string) {
  const m = list?.find((x) => x.role === role);
  const t = m?.latest?.t_ms;
  if (t == null) return false;
  const tNum = Number(t);
  if (!Number.isFinite(tNum)) return false;
  return Date.now() - tNum < DEVICE_LIVE_STALE_MS;
}

/**
 * True when this role has a `latest` frame on the shared motors list query
 * and `t_ms` is within {@link DEVICE_LIVE_STALE_MS}. Uses 1s tick so
 * liveness can flip false when frames stop, without a new query write.
 * Source: same cache WebTransport + REST update — not zustand.
 */
export function useDeviceLive(role: string): boolean {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((n) => n + 1), 1_000);
    return () => clearInterval(id);
  }, []);
  const { data: list } = useQuery({
    queryKey: queryKeys.motors.all(),
    queryFn: () => api.listMotors(),
    refetchInterval: useLiveInterval({ live: 30_000, fallback: 2_000 }),
  });
  void tick;
  return useMemo(() => isMotorLiveInList(list, role), [list, role, tick]);
}

/**
 * Wall time of the last feedback frame in the query cache, or `null` if
 * `latest` is missing. Not stored in zustand.
 */
export function useMotorLastSeenMs(role: string): number | null {
  const { data: list } = useQuery({
    queryKey: queryKeys.motors.all(),
    queryFn: () => api.listMotors(),
    refetchInterval: useLiveInterval({ live: 30_000, fallback: 2_000 }),
  });
  return useMemo(() => {
    const m = list?.find((x) => x.role === role);
    const t = m?.latest?.t_ms;
    if (t == null) return null;
    const n = Number(t);
    return Number.isFinite(n) ? n : null;
  }, [list, role]);
}

/**
 * Gating copy; 1s tick only while not live (matches prior offline-tooltip behavior).
 */
export function useDeviceOfflineTip(role: string): string {
  const isLive = useDeviceLive(role);
  const last = useMotorLastSeenMs(role);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (isLive) return;
    const id = setInterval(() => setTick((n) => n + 1), 1_000);
    return () => clearInterval(id);
  }, [isLive]);
  void tick;
  if (last == null) {
    return "No telemetry from this actuator yet — actions are disabled until it answers.";
  }
  const ageS = (Date.now() - last) / 1_000;
  return `No frame in ${ageS.toFixed(1)}s (live threshold ${DEVICE_LIVE_STALE_MS / 1_000}s).`;
}
