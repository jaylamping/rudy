// WebTransport client hook.
//
// Opens a WebTransport session to the URL advertised by `GET /api/config`
// and decodes inbound CBOR `WtEnvelope`s into typed frames. Two transports
// are read concurrently:
//
//   - QUIC datagrams (unreliable, low-latency): for high-rate "latest wins"
//     telemetry like motor feedback and system snapshots.
//   - QUIC unidirectional streams (reliable, ordered): for events that must
//     not be dropped, framed as `u32 BE length | cbor body`. The daemon
//     opens at most one reliable uni-stream per session, on demand.
//
// Both decoders feed the same per-kind dispatch table so consumers don't
// have to care which transport a stream rides. Sequence-gap detection runs
// per kind and surfaces via the `onGap` callback.
//
// IMPORTANT: this hook opens ONE QUIC session per call. The SPA mounts it
// exactly once via `<WebTransportBridge>`. Component code reads cached data
// from TanStack Query rather than subscribing here directly, so re-renders
// are throttled and the QUIC session is shared.
//
// Adding a new stream is a frontend-side one-liner: register a kind in the
// bridge's reducer registry. This file does NOT need editing.

import { decode as cborDecode } from "cbor-x/decode";
import { encode as cborEncode } from "cbor-x/encode";
import { useEffect, useRef, useState } from "react";
import type { WtSubscribe } from "@/lib/types/WtSubscribe";

/**
 * Envelope schema version this client speaks. Must match the daemon's
 * `WT_PROTOCOL_VERSION`. A mismatch surfaces as `wt.status.error`.
 */
export const WT_PROTOCOL_VERSION = 1;

/** A decoded envelope. `data` is `unknown` because the kind universe is open. */
export interface WtEnvelope<T = unknown> {
  v: number;
  kind: string;
  seq: number;
  t_ms: number | bigint;
  data: T;
}

export interface WtStatus {
  enabled: boolean;
  connected: boolean;
  error: string | null;
}

export type WtFrameListener = (frame: WtEnvelope) => void;

/**
 * Fired when the per-stream sequence counter jumps by more than 1.
 * For datagram streams this means the network or the broadcast channel
 * dropped frames; the bridge logs and continues. For reliable streams this
 * shouldn't happen; if it does it indicates a daemon bug worth reporting.
 */
export type GapListener = (gap: {
  kind: string;
  expected: number;
  got: number;
  /** How many frames were skipped (`got - expected`). */
  missed: number;
}) => void;

export interface UseWebTransportResult {
  status: WtStatus;
  /** Subscribe to every decoded envelope, regardless of kind. */
  subscribe: (listener: WtFrameListener) => () => void;
  /** Subscribe to envelopes of a specific kind. The payload type is the caller's responsibility (see WtKind). */
  onKind: <T = unknown>(
    kind: string,
    listener: (env: WtEnvelope<T>) => void,
  ) => () => void;
  /** Subscribe to per-stream sequence-gap notifications. */
  onGap: (listener: GapListener) => () => void;
  /**
   * Send (or replace) the active stream-filter `WtSubscribe`. Opens a fresh
   * bidirectional stream and writes one CBOR-encoded message; the daemon
   * applies it to the current session. Re-callable: a later send replaces
   * the previous filter.
   *
   * Returns a promise so the caller can sequence the next request after
   * the daemon has applied the filter (small but non-zero latency on
   * first call because the QUIC bidi stream init has to round-trip).
   */
  setFilter: (filter: WtSubscribe) => Promise<void>;
}

export interface UseWebTransportOptions {
  /**
   * Override the CBOR decoder. Tests use this to feed pre-built JS objects
   * through the dispatch path without exercising `cbor-x`.
   */
  decode?: (bytes: Uint8Array) => unknown;
  /**
   * Override the CBOR encoder for `setFilter`. Same rationale as `decode`.
   */
  encode?: (value: unknown) => Uint8Array;
}

export function useWebTransport(
  url: string | null | undefined,
  opts: UseWebTransportOptions = {},
): UseWebTransportResult {
  const [status, setStatus] = useState<WtStatus>({
    enabled: !!url,
    connected: false,
    error: null,
  });
  const listenersRef = useRef<Set<WtFrameListener>>(new Set());
  const gapListenersRef = useRef<Set<GapListener>>(new Set());
  // Per-kind expected next sequence. Lazily populated on first frame.
  const seqRef = useRef<Map<string, number>>(new Map());
  const decodeRef = useRef(opts.decode ?? defaultDecode);
  decodeRef.current = opts.decode ?? defaultDecode;
  const encodeRef = useRef(opts.encode ?? defaultEncode);
  encodeRef.current = opts.encode ?? defaultEncode;
  // The live transport handle, exposed via a ref so `setFilter` can reach
  // into it from outside the connect effect.
  const transportRef = useRef<WebTransport | null>(null);
  // Last applied filter; if the session reconnects we re-send it so the
  // operator's narrowing survives a transient disconnect.
  const lastFilterRef = useRef<WtSubscribe | null>(null);

  useEffect(() => {
    if (!url) {
      setStatus({ enabled: false, connected: false, error: null });
      return;
    }

    let cancelled = false;
    let transport: WebTransport | null = null;

    (async () => {
      try {
        if (
          typeof (globalThis as { WebTransport?: unknown }).WebTransport ===
          "undefined"
        ) {
          setStatus({
            enabled: true,
            connected: false,
            error: "WebTransport not supported by this browser",
          });
          return;
        }

        transport = new WebTransport(url);
        await transport.ready;
        if (cancelled) {
          transport.close();
          return;
        }
        transportRef.current = transport;
        setStatus({ enabled: true, connected: true, error: null });

        // Reset gap-detection state at the start of each session.
        seqRef.current = new Map();

        // Re-send the last filter on reconnect so a transient drop doesn't
        // silently widen the per-detail-page subscription.
        if (lastFilterRef.current) {
          await sendFilter(transport, encodeRef.current, lastFilterRef.current).catch(() => {
            /* surfaced on next call */
          });
        }

        // Drive the datagram path and the reliable-stream acceptor in
        // parallel. Both use the same dispatch helper.
        const datagramLoop = pumpDatagrams(
          transport,
          () => cancelled,
          decodeRef,
          listenersRef,
          gapListenersRef,
          seqRef,
        );
        const streamLoop = pumpReliableStreams(
          transport,
          () => cancelled,
          decodeRef,
          listenersRef,
          gapListenersRef,
          seqRef,
        );

        // If either loop throws, surface it; the other will be cancelled
        // on session close anyway.
        await Promise.race([datagramLoop, streamLoop]);
      } catch (e: unknown) {
        if (!cancelled) {
          const msg = e instanceof Error ? e.message : String(e);
          setStatus({ enabled: true, connected: false, error: msg });
        }
      }
    })();

    return () => {
      cancelled = true;
      try {
        transport?.close();
      } catch {
        // ignore
      }
      transportRef.current = null;
    };
  }, [url]);

  return {
    status,
    subscribe(listener) {
      listenersRef.current.add(listener);
      return () => {
        listenersRef.current.delete(listener);
      };
    },
    onKind<T>(kind: string, listener: (env: WtEnvelope<T>) => void) {
      const wrapped: WtFrameListener = (env) => {
        if (env.kind === kind) listener(env as WtEnvelope<T>);
      };
      listenersRef.current.add(wrapped);
      return () => {
        listenersRef.current.delete(wrapped);
      };
    },
    onGap(listener) {
      gapListenersRef.current.add(listener);
      return () => {
        gapListenersRef.current.delete(listener);
      };
    },
    async setFilter(filter) {
      lastFilterRef.current = filter;
      const t = transportRef.current;
      if (!t) {
        // Defer: the connect effect picks the filter up the moment the
        // QUIC session is ready (see lastFilterRef branch above).
        return;
      }
      await sendFilter(t, encodeRef.current, filter);
    },
  };
}

async function sendFilter(
  transport: WebTransport,
  encode: (v: unknown) => Uint8Array,
  filter: WtSubscribe,
): Promise<void> {
  const stream = await transport.createBidirectionalStream();
  const writer = stream.writable.getWriter();
  try {
    await writer.write(encode(filter));
    await writer.close();
  } finally {
    writer.releaseLock();
  }
}

function defaultEncode(value: unknown): Uint8Array {
  return cborEncode(value);
}

// ---------- internals ----------

type DecodeFn = (b: Uint8Array) => unknown;
type DecodeRef = { current: DecodeFn };
type ListenersRef = { current: Set<WtFrameListener> };
type GapListenersRef = { current: Set<GapListener> };
type SeqRef = { current: Map<string, number> };

async function pumpDatagrams(
  transport: WebTransport,
  isCancelled: () => boolean,
  decodeRef: DecodeRef,
  listenersRef: ListenersRef,
  gapListenersRef: GapListenersRef,
  seqRef: SeqRef,
): Promise<void> {
  const reader = transport.datagrams.readable.getReader();
  try {
    while (!isCancelled()) {
      const { value, done } = await reader.read();
      if (done || !value) break;
      const env = decodeEnvelope(value, decodeRef.current);
      if (env) dispatch(env, listenersRef, gapListenersRef, seqRef);
    }
  } finally {
    reader.releaseLock();
  }
}

async function pumpReliableStreams(
  transport: WebTransport,
  isCancelled: () => boolean,
  decodeRef: DecodeRef,
  listenersRef: ListenersRef,
  gapListenersRef: GapListenersRef,
  seqRef: SeqRef,
): Promise<void> {
  // The daemon opens at most one reliable uni-stream per session, lazily.
  // We loop in case the daemon ever rotates the stream (current behavior:
  // it doesn't; future: it might cap stream lifetime to bound flow-control
  // window growth). One incoming stream at a time is fine because every
  // stream is fully length-prefixed.
  const incoming = transport.incomingUnidirectionalStreams.getReader();
  try {
    while (!isCancelled()) {
      const { value: stream, done } = await incoming.read();
      if (done || !stream) break;
      try {
        await readLengthPrefixedFrames(
          stream as ReadableStream<Uint8Array>,
          isCancelled,
          decodeRef,
          listenersRef,
          gapListenersRef,
          seqRef,
        );
      } catch {
        // A misbehaving stream shouldn't kill the session; the next frame
        // will arrive on either the datagram path or a fresh stream.
      }
    }
  } finally {
    incoming.releaseLock();
  }
}

/**
 * Read `u32 BE length | bytes` frames out of a QUIC stream until it ends.
 * Maintains a small internal buffer so a frame split across multiple
 * `read()` chunks reassembles cleanly.
 */
async function readLengthPrefixedFrames(
  stream: ReadableStream<Uint8Array>,
  isCancelled: () => boolean,
  decodeRef: DecodeRef,
  listenersRef: ListenersRef,
  gapListenersRef: GapListenersRef,
  seqRef: SeqRef,
): Promise<void> {
  const reader = stream.getReader();
  let buffered: Uint8Array<ArrayBuffer> = new Uint8Array(0);

  try {
    while (!isCancelled()) {
      const { value, done } = await reader.read();
      if (done) break;
      if (value && value.byteLength > 0) {
        buffered = concat(buffered, value);
      }

      // Drain as many complete frames as we can.
      while (buffered.byteLength >= 4) {
        const len =
          (buffered[0] << 24) |
          (buffered[1] << 16) |
          (buffered[2] << 8) |
          buffered[3];
        const total = 4 + (len >>> 0);
        if (buffered.byteLength < total) break;

        const body = buffered.subarray(4, total);
        const env = decodeEnvelope(body, decodeRef.current);
        if (env) dispatch(env, listenersRef, gapListenersRef, seqRef);
        buffered = buffered.subarray(total);
      }
    }
  } finally {
    reader.releaseLock();
  }
}

function concat(a: Uint8Array, b: Uint8Array): Uint8Array<ArrayBuffer> {
  // Always allocate a fresh ArrayBuffer (vs. returning `b` when `a` is
  // empty) so the type narrows to `Uint8Array<ArrayBuffer>` rather than
  // `Uint8Array<ArrayBufferLike>`. The early-exit was a micro-opt that
  // tripped TS 5.7's stricter buffer-tracking typings.
  const out = new Uint8Array(a.byteLength + b.byteLength);
  out.set(a, 0);
  out.set(b, a.byteLength);
  return out;
}

/**
 * Decode + validate one envelope. Returns null on:
 * - malformed CBOR,
 * - missing/wrong protocol version,
 * - missing `kind` (any string is accepted; the dispatch layer drops
 *   unknown kinds by virtue of having no listener for them).
 *
 * We deliberately do NOT enforce a kind allowlist here. The daemon's
 * `declare_wt_streams!` macro is the source of truth; if a future
 * stream lands on the wire and the SPA hasn't been redeployed yet, the
 * envelope still decodes — it just has no listener and is dropped at
 * `dispatch`. That's strictly better than silently logging "unknown
 * kind" warnings every frame for a stream the user opted into.
 */
function decodeEnvelope(
  bytes: Uint8Array,
  decode: (b: Uint8Array) => unknown,
): WtEnvelope | null {
  let value: unknown;
  try {
    value = decode(bytes);
  } catch {
    return null;
  }
  if (!value || typeof value !== "object") return null;
  const v = (value as { v?: unknown }).v;
  const kind = (value as { kind?: unknown }).kind;
  const seq = (value as { seq?: unknown }).seq;
  if (typeof v !== "number" || v !== WT_PROTOCOL_VERSION) return null;
  if (typeof kind !== "string") return null;
  if (typeof seq !== "number" && typeof seq !== "bigint") return null;
  // The shape matches; trust the rest of the fields. The codec test on
  // the Rust side guarantees them; if the daemon is sending garbage
  // payloads, downstream consumers will surface NaN/undefined and the
  // codec test would have caught it in CI.
  return value as WtEnvelope;
}

function dispatch(
  env: WtEnvelope,
  listenersRef: ListenersRef,
  gapListenersRef: GapListenersRef,
  seqRef: SeqRef,
): void {
  // Per-kind gap detection. Sequence is monotonic per kind; the first
  // frame seeds the counter (any starting seq is fine).
  const seq = Number(env.seq);
  const expected = seqRef.current.get(env.kind);
  if (expected !== undefined && seq !== expected) {
    if (seq > expected) {
      const missed = seq - expected;
      for (const g of gapListenersRef.current) {
        g({ kind: env.kind, expected, got: seq, missed });
      }
    }
    // For seq < expected (reorder or wrap): silently accept and re-anchor.
    // Datagrams can reorder; we don't want to bin those frames.
  }
  seqRef.current.set(env.kind, seq + 1);

  for (const l of listenersRef.current) l(env);
}

function defaultDecode(bytes: Uint8Array): unknown {
  return cborDecode(bytes);
}
