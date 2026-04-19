// Module-level handle on the WebTransport bridge so consumers outside of
// React (or in components mounted after the bridge) can subscribe to extra
// streams without re-opening a QUIC session.
//
// The bridge writes its `useWebTransport` result here on every render via
// `publishBridgeWt`. Hooks consume it through `getBridgeWt()`.
//
// Why a module global instead of React Context: keeping the bridge result
// out of context means a route switch doesn't re-render every consumer
// just because the bridge re-rendered. Consumers that genuinely care about
// connection status read `useWtStatus()` separately.

import type { UseWebTransportResult } from "@/lib/hooks/useWebTransport";

let current: UseWebTransportResult | null = null;

/** Bridge-only: replace the published handle. */
export function publishBridgeWt(next: UseWebTransportResult): void {
  current = next;
}

/** Returns the live `useWebTransport` result, or null if no bridge is mounted. */
export function getBridgeWt(): UseWebTransportResult | null {
  return current;
}
