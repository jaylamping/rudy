// Bridges the WebTransport firehose into the TanStack Query cache.
//
// Mounted exactly once at the router root. Each registered reducer maps a
// stream kind to a function that updates the relevant query cache entry.
// A single requestAnimationFrame flush per tick fires every reducer that
// has buffered work, so the UI re-renders at most once per display frame
// regardless of wire-rate.
//
// Why a registry: adding a new "near-realtime" stream is now a one-file
// change — write a reducer, register it. The bridge has no per-stream
// branches; the hard-coded motor/system reducers are just two entries in
// the default registry.
//
// Why rAF: the daemon broadcasts at telemetry.poll_interval_ms (default
// 100 ms ≈ 10 Hz × N motors). Without coalescing, every datagram triggers
// a React re-render across every subscriber of the relevant query, which
// melts the main thread on a ~10-motor inventory + uPlot grid. rAF caps
// cache writes at the display refresh rate (≤60 Hz) regardless of how
// fast the wire fires; the UI still feels live because each render
// carries the freshest value seen.
//
// Why this lives outside `useWebTransport`: keeping the QUIC session in
// one place means a route switch doesn't tear down the connection, and
// tests of the hook don't have to spin up a QueryClient.
//
// The bridge also publishes its connection status via `useWtStatus()` so
// query consumers can pick a poll cadence (slow safety net when WT is
// up, fast REST poll when it's down). See `useWtConnected`.

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "@/lib/api";
import { useWebTransport } from "@/lib/hooks/useWebTransport";
import { publishBridgeWt } from "@/lib/hooks/wt-bridge-handle";
import { publishWtStatus } from "@/lib/hooks/wt-status";
import { DEFAULT_REDUCERS, type WtReducer } from "@/lib/hooks/wt-reducers";
import type { ServerConfig } from "@/lib/types/ServerConfig";

export type { WtReducer } from "@/lib/hooks/wt-reducers";

interface BridgeProps {
  /**
   * Override the WT URL. By default the bridge fetches `/api/config` to
   * discover whether WebTransport is enabled and where it's bound. The
   * override exists for tests and Storybook-style harnesses.
   */
  urlOverride?: string | null;
  /**
   * Custom reducer registry. Defaults to motor-feedback + system-snapshot.
   * Pass an extended array to wire up new streams without forking the
   * bridge. Test harnesses pass `[]` to drive the WT plumbing without
   * reducers.
   */
  reducers?: WtReducer[];
  /**
   * Optional gap-detection callback. Defaults to a `console.warn`. Tests
   * pass a spy.
   */
  onGap?: (gap: {
    kind: string;
    expected: number;
    got: number;
    missed: number;
  }) => void;
}

export function WebTransportBridge({
  urlOverride,
  reducers,
  onGap,
}: BridgeProps = {}) {
  const queryClient = useQueryClient();

  // Discover the WT advert. We fetch `/api/config` here — the dashboard's
  // ConnectionCard fetches it too, but TanStack Query dedupes by ["config"]
  // queryKey so the network only sees one request.
  const [cfg, setCfg] = useState<ServerConfig | null>(null);
  useEffect(() => {
    if (urlOverride !== undefined) return;
    let cancelled = false;
    queryClient
      .fetchQuery({
        queryKey: ["config"],
        queryFn: () => api.config(),
        staleTime: 60_000,
      })
      .then((c) => {
        if (!cancelled) setCfg(c);
      })
      .catch(() => {
        // Config fetch failure is non-fatal; the bridge stays disabled
        // and existing REST polls keep the dashboard alive.
      });
    return () => {
      cancelled = true;
    };
  }, [queryClient, urlOverride]);

  const url =
    urlOverride !== undefined
      ? urlOverride
      : cfg?.webtransport.enabled
        ? (cfg.webtransport.url ?? null)
        : null;

  const wt = useWebTransport(url);

  useEffect(() => {
    publishWtStatus(wt.status);
  }, [wt.status]);

  // Publish the live `useWebTransport` result so non-bridge consumers
  // (e.g. the per-run `test_progress` listener inside the Tests tab) can
  // attach extra `onKind` listeners without re-opening a QUIC session.
  useEffect(() => {
    publishBridgeWt(wt);
  }, [wt]);

  // The active registry. Memoize on identity of the prop so a parent that
  // builds a fresh array each render doesn't tear down subscriptions.
  const activeReducers = useMemo(
    () => reducers ?? DEFAULT_REDUCERS,
    [reducers],
  );

  // Bucket-per-reducer; the bridge owns these between flushes. We re-init
  // when the reducer set changes so a swap doesn't carry over stale state.
  const bucketsRef = useRef<Map<string, unknown>>(new Map());
  const dirtyRef = useRef<Set<string>>(new Set());
  const rafHandleRef = useRef<number | null>(null);

  // Reset buckets when reducer set changes.
  useEffect(() => {
    const next = new Map<string, unknown>();
    for (const r of activeReducers) next.set(r.kind, r.initBucket());
    bucketsRef.current = next;
    dirtyRef.current = new Set();
  }, [activeReducers]);

  useEffect(() => {
    const flush = () => {
      rafHandleRef.current = null;
      for (const r of activeReducers) {
        if (!dirtyRef.current.has(r.kind)) continue;
        const bucket = bucketsRef.current.get(r.kind);
        if (bucket === undefined) continue;
        try {
          r.flush(bucket, queryClient);
        } catch (e) {
          console.error(`wt: reducer flush failed for kind=${r.kind}`, e);
        }
        const reset = r.resetBucket
          ? r.resetBucket(bucket)
          : r.initBucket();
        bucketsRef.current.set(r.kind, reset);
      }
      dirtyRef.current.clear();
    };

    const schedule = () => {
      if (rafHandleRef.current !== null) return;
      // requestAnimationFrame is undefined in vitest's jsdom; fall back to
      // a microtask-equivalent so unit tests can drive the bridge end-to-end.
      const raf =
        typeof requestAnimationFrame === "function"
          ? requestAnimationFrame
          : (cb: FrameRequestCallback): number => {
              const t = setTimeout(() => cb(performance.now()), 0);
              return t as unknown as number;
            };
      rafHandleRef.current = raf(flush);
    };

    // Single subscription that fans out to the right reducer by kind.
    // Cheaper than N onKind subscriptions because the dispatch is one
    // Map lookup instead of N comparisons.
    const reducerByKind = new Map<string, WtReducer>(
      activeReducers.map((r) => [r.kind, r]),
    );
    const unsub = wt.subscribe((env) => {
      const r = reducerByKind.get(env.kind);
      if (!r) return; // unknown kind; harmless drop (see useWebTransport)
      const bucket = bucketsRef.current.get(env.kind);
      if (bucket === undefined) return;
      const dirty = r.merge(bucket, env);
      if (dirty) {
        dirtyRef.current.add(env.kind);
        schedule();
      }
    });

    const unsubGap = wt.onGap(
      onGap ??
        ((gap) => {
          // Default behavior: log once per gap. The bridge keeps running.
          console.warn(
            `wt: stream gap kind=${gap.kind} expected=${gap.expected} got=${gap.got} missed=${gap.missed}`,
          );
        }),
    );

    return () => {
      unsub();
      unsubGap();
      if (rafHandleRef.current !== null) {
        const cancel =
          typeof cancelAnimationFrame === "function"
            ? cancelAnimationFrame
            : (h: number) => clearTimeout(h);
        cancel(rafHandleRef.current);
        rafHandleRef.current = null;
      }
    };
    // wt is a fresh object each render; re-binding the listener every
    // render is cheap (Set add/remove) and ensures we always read from
    // the live session.
  }, [wt, queryClient, activeReducers, onGap]);

  return null;
}
