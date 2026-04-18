// Pub/sub for the WebTransport bridge's connection status.
//
// Lives in its own module (separate from `WebTransportBridge.tsx`) so the
// `react-refresh` plugin can keep fast-refreshing the bridge component
// without tearing down hook subscribers, and so consumers don't have to
// import a component file just to read connection state.
//
// There is exactly one writer (the bridge) and many readers (`useLiveInterval`
// today, dashboards / status pills tomorrow). A tiny Set-of-listeners pattern
// is enough; we deliberately avoid React Context to keep status changes from
// re-rendering every route.

import { useEffect, useState } from "react";
import type { WtStatus } from "@/lib/hooks/useWebTransport";

type StatusListener = (s: WtStatus) => void;

const listeners = new Set<StatusListener>();
let current: WtStatus = { enabled: false, connected: false, error: null };

/** Bridge-only: replace the published status if it changed. */
export function publishWtStatus(next: WtStatus): void {
  if (
    next.enabled === current.enabled &&
    next.connected === current.connected &&
    next.error === current.error
  ) {
    return;
  }
  current = next;
  for (const l of listeners) l(next);
}

/** Subscribe to status changes; returns the live `WtStatus`. */
export function useWtStatus(): WtStatus {
  const [s, setS] = useState<WtStatus>(current);
  useEffect(() => {
    listeners.add(setS);
    setS(current);
    return () => {
      listeners.delete(setS);
    };
  }, []);
  return s;
}

/** Convenience: just the boolean. */
export function useWtConnected(): boolean {
  return useWtStatus().connected;
}
