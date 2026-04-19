// Subscribe to the WebTransport `test_progress` stream filtered to one
// run id. Returns a stable, ordered array of `TestProgress` lines.
//
// The hook does NOT manage the underlying QUIC session; that's the
// `<WebTransportBridge>`'s job. We attach via `getBridgeWt().onKind()` and
// keep our own per-run buffer here. Listener is dropped automatically when
// `runId` flips back to null (no run in progress).
//
// We also push a `WtSubscribe` filter so the daemon only sends
// `test_progress` frames for this run + always-on telemetry kinds.
// `motor_feedback` is included so the rest of the SPA keeps getting live
// data while the Tests tab is open. Restored to "all kinds" on unmount.

import { useEffect, useState } from "react";
import { getBridgeWt } from "@/lib/hooks/wtBridgeHandle";
import type { WtEnvelope } from "@/lib/hooks/useWebTransport";
import type { TestProgress } from "@/lib/types/TestProgress";

export function useTestProgress(runId: string | null): TestProgress[] {
  const [lines, setLines] = useState<TestProgress[]>([]);

  useEffect(() => {
    setLines([]);
    if (!runId) return;
    const wt = getBridgeWt();
    if (!wt) return;

    void wt.setFilter({
      kinds: ["motor_feedback", "system_snapshot", "test_progress", "safety_event"],
      filters: { motor_roles: [], run_ids: [runId] },
    });

    const off = wt.onKind<TestProgress>("test_progress", (env: WtEnvelope<TestProgress>) => {
      if (env.data.run_id !== runId) return;
      setLines((prev) => [...prev, env.data]);
    });

    return () => {
      off();
      // Restore the implicit "all kinds, no filter" subscription so other
      // tabs aren't permanently narrowed by the test we just ran.
      void wt.setFilter({
        kinds: [],
        filters: { motor_roles: [], run_ids: [] },
      });
    };
  }, [runId]);

  return lines;
}
