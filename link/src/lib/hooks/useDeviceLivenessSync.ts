import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { queryKeys } from "@/api";
import { api } from "@/lib/api";
import { DEVICE_LIVE_STALE_MS } from "@/lib/deviceLiveness";
import { useLiveInterval } from "@/lib/hooks/useLiveInterval";
import type { DeviceLiveness } from "@/store/slices/deviceSlice";
import { useStore } from "@/store";

/**
 * Single place that projects the motors query into per-role liveness.
 * Mount once under the app shell so any route can `useDeviceLive(role)`.
 *
 * Re-evaluates on a 1s tick so a motor that goes quiet crosses the stale
 * threshold without waiting for the next HTTP refetch.
 */
export function useDeviceLivenessSync() {
  const setMany = useStore((s) => s.setManyDeviceLiveness);
  const tick = useTick(1_000);

  const motorsQ = useQuery({
    queryKey: queryKeys.motors.all(),
    queryFn: () => api.listMotors(),
    refetchInterval: useLiveInterval({ live: 30_000, fallback: 2_000 }),
  });

  useEffect(() => {
    if (!motorsQ.data) return;
    const now = Date.now();
    const next: Record<string, DeviceLiveness> = {};
    for (const m of motorsQ.data) {
      const lastSeen = m.latest ? Number(m.latest.t_ms) : null;
      next[m.role] = {
        isOnline: lastSeen != null && now - lastSeen < DEVICE_LIVE_STALE_MS,
        lastSeenMs: lastSeen,
      };
    }
    setMany(next);
  }, [motorsQ.data, setMany, tick]);
}

function useTick(ms: number) {
  const [n, setN] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setN((x) => x + 1), ms);
    return () => clearInterval(id);
  }, [ms]);
  return n;
}
