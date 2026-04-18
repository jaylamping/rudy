// Tests for the WebTransport client hook.
//
// Per the request thread we deliberately do NOT exercise the QUIC transport
// layer end-to-end — that's covered by manual smoke + bench tests against a
// real Pi. What we DO pin here is the hook's status state machine, since the
// `<TelemetryGrid />` UI reads `WtStatus` directly to decide whether to fall
// back to REST polling.
//
// We also pin the empty-input contract: passing `null` / `undefined` for the
// URL (which is what `api.config().webtransport.url` returns when WT is
// disabled server-side) must produce `enabled: false` and never call into the
// WebTransport constructor.

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useWebTransport } from "@/lib/hooks/useWebTransport";

class StubReadableStreamReader {
  // Simple FIFO reader that completes when `close()` is called.
  private queue: Uint8Array[] = [];
  private closed = false;
  private resolveNext: ((v: ReadableStreamReadResult<Uint8Array>) => void) | null =
    null;
  push(bytes: Uint8Array) {
    if (this.resolveNext) {
      const r = this.resolveNext;
      this.resolveNext = null;
      r({ value: bytes, done: false });
    } else {
      this.queue.push(bytes);
    }
  }
  close() {
    this.closed = true;
    if (this.resolveNext) {
      this.resolveNext({ value: undefined, done: true });
      this.resolveNext = null;
    }
  }
  read(): Promise<ReadableStreamReadResult<Uint8Array>> {
    if (this.queue.length > 0) {
      return Promise.resolve({ value: this.queue.shift()!, done: false });
    }
    if (this.closed) {
      return Promise.resolve({ value: undefined, done: true });
    }
    return new Promise((resolve) => {
      this.resolveNext = resolve;
    });
  }
  releaseLock() {
    /* no-op for the stub */
  }
}

class StubWebTransport {
  static last: StubWebTransport | null = null;
  static neverResolve = false;
  ready: Promise<void>;
  closed = false;
  readonly reader = new StubReadableStreamReader();
  readonly datagrams = {
    readable: { getReader: () => this.reader },
  };
  constructor(public readonly url: string) {
    StubWebTransport.last = this;
    this.ready = StubWebTransport.neverResolve
      ? new Promise(() => {})
      : Promise.resolve();
  }
  close() {
    this.closed = true;
    this.reader.close();
  }
}

beforeEach(() => {
  StubWebTransport.last = null;
  StubWebTransport.neverResolve = false;
  (globalThis as unknown as { WebTransport: typeof StubWebTransport }).WebTransport =
    StubWebTransport;
});

afterEach(() => {
  delete (globalThis as { WebTransport?: unknown }).WebTransport;
  vi.restoreAllMocks();
});

describe("useWebTransport", () => {
  it("reports enabled=false when no URL is provided (server WT disabled)", async () => {
    const { result } = renderHook(() => useWebTransport(null));
    await waitFor(() => {
      expect(result.current.status).toEqual({
        enabled: false,
        connected: false,
        error: null,
      });
    });
    expect(StubWebTransport.last).toBeNull();
  });

  it("opens a session and flips to connected once `transport.ready` resolves", async () => {
    const { result } = renderHook(() =>
      useWebTransport("https://rudy.example.ts.net:4433/wt"),
    );
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });
    expect(result.current.status).toEqual({
      enabled: true,
      connected: true,
      error: null,
    });
    expect(StubWebTransport.last?.url).toBe(
      "https://rudy.example.ts.net:4433/wt",
    );
  });

  it("surfaces an error when the browser doesn't support WebTransport", async () => {
    delete (globalThis as { WebTransport?: unknown }).WebTransport;
    const { result } = renderHook(() => useWebTransport("https://x/wt"));
    await waitFor(() => {
      expect(result.current.status.error).toMatch(/not supported/i);
    });
    expect(result.current.status).toEqual({
      enabled: true,
      connected: false,
      error: expect.stringMatching(/not supported/i),
    });
  });

  it("surfaces an error when the WebTransport ready handshake throws", async () => {
    (globalThis as unknown as { WebTransport: unknown }).WebTransport = class {
      ready = Promise.reject(new Error("certificate hash mismatch"));
      close() {}
      datagrams = { readable: { getReader: () => ({ read: () => Promise.resolve({ done: true }), releaseLock() {} }) } };
    };

    const { result } = renderHook(() => useWebTransport("https://x/wt"));
    await waitFor(() => {
      expect(result.current.status.error).toMatch(/certificate hash mismatch/);
    });
    expect(result.current.status.connected).toBe(false);
  });

  it("subscribe() returns an unsubscribe function and never-fires today (decoder is a stub)", async () => {
    // Documents Phase-1 behaviour: even though datagrams arrive on the
    // transport, decodeFeedback() returns null until a real CBOR decoder
    // lands. When that ships, this test will need to start asserting the
    // listener fires.
    const { result } = renderHook(() => useWebTransport("https://x/wt"));
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });

    const listener = vi.fn();
    let unsubscribe: () => void = () => {};
    act(() => {
      unsubscribe = result.current.subscribe(listener);
    });

    // Pump a fake datagram through; current stub decoder discards it.
    StubWebTransport.last!.reader.push(new Uint8Array([0xa0]));
    // Yield so the read loop processes it.
    await new Promise((r) => setTimeout(r, 10));

    expect(listener).not.toHaveBeenCalled();
    expect(typeof unsubscribe).toBe("function");
    unsubscribe();
  });

  it("closes the transport when the hook unmounts", async () => {
    const { result, unmount } = renderHook(() => useWebTransport("https://x/wt"));
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });
    const transport = StubWebTransport.last!;
    unmount();
    expect(transport.closed).toBe(true);
  });
});
