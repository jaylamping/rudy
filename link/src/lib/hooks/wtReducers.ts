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
import { queryKeys } from "@/api";
import type { WtEnvelope } from "@/lib/hooks/useWebTransport";
import type { LogEntry } from "@/lib/types/LogEntry";
import type { MotorFeedback } from "@/lib/types/MotorFeedback";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import type { SystemSnapshot } from "@/lib/types/SystemSnapshot";

/** Cap on the in-memory live tail. Older entries fall off the front; the
 * full history is still queryable via `GET /api/logs`, so the cap is
 * about UI memory pressure, not durability. 5000 is large enough for a
 * developer to scroll through a few seconds of debug-level chatter
 * without the page going laggy. */
export const LIVE_LOG_CAP = 5000;

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
    queryClient.setQueryData<MotorSummary[]>(queryKeys.motors.all(), (prev) => {
      // The motors list (inventory metadata) must come from the REST
      // bootstrap before we can merge live frames. If it isn't seeded
      // yet, drop the updates: the next motor frame will retry, and
      // the initial `useQuery({queryKey: queryKeys.motors.all()})` populates the
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
        return { ...m, latest: fb, feedback_age_ms: 0n, type2_age_ms: 0n };
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
    queryClient.setQueryData<SystemSnapshot>(queryKeys.system(), (prev) => {
      if (prev && Number(prev.t_ms) >= Number(snap.t_ms)) return prev;
      return snap;
    });
  },
};

/**
 * Append-and-cap merge for `log_event`. Bucket holds the live tail so
 * the Logs page can render new entries without re-fetching `/api/logs`
 * after every event. We coalesce per-rAF (the rest of the registry's
 * convention) which keeps a 1 kHz debug-tracing burst from re-rendering
 * the page every frame.
 *
 * The cache contract: query key `["logs", "live"]` is an array of
 * `LogEntry`s in newest-first order, capped at `LIVE_LOG_CAP`. The
 * Logs page seeds the cache with a REST page so the live tail and the
 * historical view share one list.
 */
const logEventReducer: WtReducer<LogEntry, { incoming: LogEntry[] }> = {
  kind: "log_event",
  initBucket: () => ({ incoming: [] }),
  merge(bucket, env) {
    bucket.incoming.push(env.data);
    return true;
  },
  flush(bucket, queryClient) {
    if (bucket.incoming.length === 0) return;
    // Fresh entries arrive in submission order (oldest first within the
    // burst); the cache stores newest-first. Reverse the burst then
    // prepend so the cache stays sorted without resorting the whole tail.
    const burst = bucket.incoming.slice().reverse();
    queryClient.setQueryData<LogEntry[]>(queryKeys.logs.live(), (prev) => {
      const base = prev ?? [];
      const merged = burst.concat(base);
      return merged.length > LIVE_LOG_CAP ? merged.slice(0, LIVE_LOG_CAP) : merged;
    });
  },
  resetBucket: () => ({ incoming: [] }),
};

/** Default registry mounted by `<WebTransportBridge>` when no override is given. */
export const DEFAULT_REDUCERS: WtReducer[] = [
  motorFeedbackReducer as unknown as WtReducer,
  systemSnapshotReducer as unknown as WtReducer,
  logEventReducer as unknown as WtReducer,
];
