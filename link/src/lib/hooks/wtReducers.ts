// Default WebTransport reducer registry.
//
// Lives here (rather than in WebTransportBridge.tsx) so React Fast Refresh
// works cleanly: the bridge file only exports a component, this file only
// exports plain functions/constants. Adding a new "near-realtime" stream
// to the dashboard means: add a payload type to types.rs (via
// `declare_wt_streams!`), then append a reducer here.
//
// Each reducer is one entry in the registry passed to `<WebTransportBridge
// reducers={...}>`. See the `WtReducer` interface for the shape contract;
// the short version is `merge(bucket, env)` runs at wire-rate, `flush(bucket,
// queryClient)` runs once per requestAnimationFrame.

import type { QueryClient } from "@tanstack/react-query";
import type { WtEnvelope } from "@/lib/hooks/useWebTransport";
import type { MotorFeedback } from "@/lib/types/MotorFeedback";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import type { SystemSnapshot } from "@/lib/types/SystemSnapshot";

export interface WtReducer<TPayload = unknown, TBucket = unknown> {
  /** snake_case kind (matches the daemon's `WtPayload::KIND`). */
  kind: string;
  /** Initial empty bucket. Called once per session, not per frame. */
  initBucket: () => TBucket;
  /** Merge one envelope into the bucket. Return `true` to schedule a flush. */
  merge: (bucket: TBucket, env: WtEnvelope<TPayload>) => boolean;
  /** Called once per rAF tick when `merge` returned true at least once. */
  flush: (bucket: TBucket, queryClient: QueryClient) => void;
  /** Reset the bucket after `flush`. Defaults to calling `initBucket`. */
  resetBucket?: (bucket: TBucket) => TBucket;
}

/**
 * Latest-wins-per-role merge for `motor_feedback`. Bucket is a Map keyed
 * by role; flush walks the inventory in `["motors"]` and swaps each
 * motor's `latest` for the freshest WT sample, preserving the existing
 * recency guard against out-of-order writes.
 */
const motorFeedbackReducer: WtReducer<
  MotorFeedback,
  Map<string, MotorFeedback>
> = {
  kind: "motor_feedback",
  initBucket: () => new Map(),
  merge(bucket, env) {
    bucket.set(env.data.role, env.data);
    return true;
  },
  flush(bucket, queryClient) {
    if (bucket.size === 0) return;
    queryClient.setQueryData<MotorSummary[]>(["motors"], (prev) => {
      // The motors list (inventory metadata) must come from the REST
      // bootstrap before we can merge live frames. If it isn't seeded
      // yet, drop the updates: the next motor frame will retry, and
      // the initial `useQuery({queryKey: ["motors"]})` populates the
      // baseline within ~one network RTT.
      if (!prev) return prev;
      let changed = false;
      const next = prev.map((m) => {
        const fb = bucket.get(m.role);
        if (!fb) return m;
        if (m.latest && Number(m.latest.t_ms) >= Number(fb.t_ms)) {
          return m;
        }
        changed = true;
        return { ...m, latest: fb };
      });
      return changed ? next : prev;
    });
  },
};

/**
 * Replace-on-arrival merge for `system_snapshot`. Bucket is the single
 * newest envelope; flush writes it to `["system"]` if it's strictly
 * fresher than what's there.
 */
const systemSnapshotReducer: WtReducer<
  SystemSnapshot,
  { latest: SystemSnapshot | null }
> = {
  kind: "system_snapshot",
  initBucket: () => ({ latest: null }),
  merge(bucket, env) {
    bucket.latest = env.data;
    return true;
  },
  flush(bucket, queryClient) {
    if (!bucket.latest) return;
    const snap = bucket.latest;
    queryClient.setQueryData<SystemSnapshot>(["system"], (prev) => {
      if (prev && Number(prev.t_ms) >= Number(snap.t_ms)) return prev;
      return snap;
    });
  },
};

/** Default registry mounted by `<WebTransportBridge>` when no override is given. */
export const DEFAULT_REDUCERS: WtReducer[] = [
  motorFeedbackReducer as unknown as WtReducer,
  systemSnapshotReducer as unknown as WtReducer,
];
