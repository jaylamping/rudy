// Pick a TanStack Query `refetchInterval` based on whether the WebTransport
// bridge is currently piping live data into the cache.
//
// When WT is connected, the cache is being updated every animation frame
// from the firehose; we only need REST as a slow safety net (e.g. to recover
// from a missed datagram or to seed a freshly-mounted route). When WT is
// disconnected (Vite dev without TLS, non-Chromium browser, server flag
// off), REST has to carry the full liveness load.
//
// Usage:
//   useQuery({
//     queryKey: ["motors"],
//     queryFn: () => api.listMotors(),
//     refetchInterval: useLiveInterval({ live: 30_000, fallback: 1_000 }),
//   });

import { useWtConnected } from "@/lib/hooks/wtStatus";

export interface LiveIntervalOpts {
  /** Slow safety-net cadence to use when the WT bridge is connected. */
  live: number;
  /** Aggressive cadence to use when WT isn't connected. */
  fallback: number;
}

export function useLiveInterval({ live, fallback }: LiveIntervalOpts): number {
  return useWtConnected() ? live : fallback;
}
