// Tests for the WebTransport client hook.
//
// We deliberately do NOT exercise the QUIC transport layer end-to-end —
// that's covered by manual smoke + bench tests against a real Pi. What we
// pin here is:
//   - the hook's status state machine (the bridge + REST fallback rely on it)
//   - the envelope decoder (rejects wrong protocol version, missing fields)
//   - the per-kind dispatch via `onKind`
//   - tolerance to malformed datagrams (one bad frame must not kill the loop)
//   - per-stream sequence-gap detection via `onGap`
//   - the reliable-stream length-prefixed reader

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  useWebTransport,
  WT_PROTOCOL_VERSION,
  type WtEnvelope,
} from "@/lib/hooks/useWebTransport";

// ---------- stub WebTransport plumbing ----------

class StubReadableStreamReader<T> {
  private queue: T[] = [];
  private closed = false;
  private resolveNext: ((v: ReadableStreamReadResult<T>) => void) | null = null;
  push(v: T) {
    if (this.resolveNext) {
      const r = this.resolveNext;
      this.resolveNext = null;
      r({ value: v, done: false });
    } else {
      this.queue.push(v);
    }
  }
  close() {
    this.closed = true;
    if (this.resolveNext) {
      this.resolveNext({ value: undefined, done: true });
      this.resolveNext = null;
    }
  }
  read(): Promise<ReadableStreamReadResult<T>> {
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
  releaseLock() {}
}

class StubWebTransport {
  static last: StubWebTransport | null = null;
  static neverResolve = false;
  ready: Promise<void>;
  closed = false;
  readonly datagramReader = new StubReadableStreamReader<Uint8Array>();
  readonly streamReader = new StubReadableStreamReader<ReadableStream<Uint8Array>>();
  readonly datagrams = {
    readable: { getReader: () => this.datagramReader },
  };
  readonly incomingUnidirectionalStreams = {
    getReader: () => this.streamReader,
  };
  constructor(public readonly url: string) {
    StubWebTransport.last = this;
    this.ready = StubWebTransport.neverResolve
      ? new Promise(() => {})
      : Promise.resolve();
  }
  close() {
    this.closed = true;
    this.datagramReader.close();
    this.streamReader.close();
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

// Build a well-formed envelope for the stub decoder to return. The decoder
// is opaque; the hook only inspects the envelope shape.
function envelope<T>(kind: string, seq: number, data: T): WtEnvelope<T> {
  return {
    v: WT_PROTOCOL_VERSION,
    kind,
    seq,
    t_ms: Date.now(),
    data,
  };
}

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
  });

  it("surfaces an error when the WebTransport ready handshake throws", async () => {
    (globalThis as unknown as { WebTransport: unknown }).WebTransport = class {
      ready = Promise.reject(new Error("certificate hash mismatch"));
      close() {}
      datagrams = {
        readable: {
          getReader: () => ({
            read: () => Promise.resolve({ done: true }),
            releaseLock() {},
          }),
        },
      };
      incomingUnidirectionalStreams = {
        getReader: () => ({
          read: () => Promise.resolve({ done: true }),
          releaseLock() {},
        }),
      };
    };

    const { result } = renderHook(() => useWebTransport("https://x/wt"));
    await waitFor(() => {
      expect(result.current.status.error).toMatch(/certificate hash mismatch/);
    });
  });

  it("decodes datagrams and dispatches by `kind`", async () => {
    const fixtures = [
      envelope("motor_feedback", 0, { role: "shoulder_a", pos: 0.1 }),
      envelope("system_snapshot", 0, { hostname: "test" }),
    ];
    let cursor = 0;
    const decode = (_: Uint8Array) => fixtures[cursor++];

    const { result } = renderHook(() =>
      useWebTransport("https://x/wt", { decode }),
    );
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });

    const onMotor = vi.fn();
    const onSystem = vi.fn();
    const onAll = vi.fn();
    act(() => {
      result.current.onKind<{ role: string; pos: number }>(
        "motor_feedback",
        onMotor,
      );
      result.current.onKind<{ hostname: string }>("system_snapshot", onSystem);
      result.current.subscribe(onAll);
    });

    StubWebTransport.last!.datagramReader.push(new Uint8Array([0]));
    StubWebTransport.last!.datagramReader.push(new Uint8Array([0]));
    await new Promise((r) => setTimeout(r, 30));

    expect(onMotor).toHaveBeenCalledTimes(1);
    expect(onMotor.mock.calls[0][0].data.role).toBe("shoulder_a");
    expect(onSystem).toHaveBeenCalledTimes(1);
    expect(onSystem.mock.calls[0][0].data.hostname).toBe("test");
    expect(onAll).toHaveBeenCalledTimes(2);
  });

  it("rejects envelopes with the wrong protocol version", async () => {
    let cursor = 0;
    const fixtures = [
      // Wrong v
      { v: 99, kind: "motor_feedback", seq: 0, t_ms: 0, data: { role: "x" } },
      // Correct v
      envelope("motor_feedback", 0, { role: "y" }),
    ];
    const decode = (_: Uint8Array): unknown => fixtures[cursor++];

    const { result } = renderHook(() =>
      useWebTransport("https://x/wt", { decode }),
    );
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });

    const onMotor = vi.fn();
    act(() => {
      result.current.onKind<{ role: string }>("motor_feedback", onMotor);
    });

    StubWebTransport.last!.datagramReader.push(new Uint8Array([0]));
    StubWebTransport.last!.datagramReader.push(new Uint8Array([0]));
    await new Promise((r) => setTimeout(r, 30));

    expect(onMotor).toHaveBeenCalledTimes(1);
    expect(onMotor.mock.calls[0][0].data.role).toBe("y");
  });

  it("drops malformed datagrams without breaking the read loop", async () => {
    // 1: decoder throws. 2: missing kind. 3: valid frame.
    const calls: number[] = [];
    const decode = (_: Uint8Array): unknown => {
      calls.push(calls.length);
      if (calls.length === 1) throw new Error("bad cbor");
      if (calls.length === 2) return { v: WT_PROTOCOL_VERSION, seq: 0, t_ms: 0 };
      return envelope("motor_feedback", 0, { role: "ok" });
    };

    const { result } = renderHook(() =>
      useWebTransport("https://x/wt", { decode }),
    );
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });

    const onMotor = vi.fn();
    act(() => {
      result.current.onKind<{ role: string }>("motor_feedback", onMotor);
    });

    StubWebTransport.last!.datagramReader.push(new Uint8Array([0x01]));
    StubWebTransport.last!.datagramReader.push(new Uint8Array([0x02]));
    StubWebTransport.last!.datagramReader.push(new Uint8Array([0x03]));
    await new Promise((r) => setTimeout(r, 30));

    expect(calls).toEqual([0, 1, 2]);
    expect(onMotor).toHaveBeenCalledTimes(1);
    expect(onMotor.mock.calls[0][0].data.role).toBe("ok");
  });

  it("typed listeners can be unsubscribed", async () => {
    const decode = (_: Uint8Array): unknown =>
      envelope("motor_feedback", 0, { role: "x" });

    const { result } = renderHook(() =>
      useWebTransport("https://x/wt", { decode }),
    );
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });

    const onMotor = vi.fn();
    let unsubscribe: () => void = () => {};
    act(() => {
      unsubscribe = result.current.onKind("motor_feedback", onMotor);
    });

    StubWebTransport.last!.datagramReader.push(new Uint8Array([0]));
    await new Promise((r) => setTimeout(r, 30));
    expect(onMotor).toHaveBeenCalledTimes(1);

    unsubscribe();
    StubWebTransport.last!.datagramReader.push(new Uint8Array([0]));
    await new Promise((r) => setTimeout(r, 30));
    expect(onMotor).toHaveBeenCalledTimes(1);
  });

  it("fires onGap when a kind's sequence skips ahead", async () => {
    let cursor = 0;
    const seqs = [10, 11, 15]; // gap between 11 and 15: missed 3
    const decode = (_: Uint8Array): unknown =>
      envelope("motor_feedback", seqs[cursor++], { role: "x" });

    const { result } = renderHook(() =>
      useWebTransport("https://x/wt", { decode }),
    );
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });

    const onGap = vi.fn();
    act(() => {
      result.current.onGap(onGap);
    });

    StubWebTransport.last!.datagramReader.push(new Uint8Array([0]));
    StubWebTransport.last!.datagramReader.push(new Uint8Array([0]));
    StubWebTransport.last!.datagramReader.push(new Uint8Array([0]));
    await new Promise((r) => setTimeout(r, 30));

    expect(onGap).toHaveBeenCalledTimes(1);
    expect(onGap.mock.calls[0][0]).toEqual({
      kind: "motor_feedback",
      expected: 12,
      got: 15,
      missed: 3,
    });
  });

  it("does not fire onGap for the very first frame of a kind", async () => {
    const decode = (_: Uint8Array): unknown =>
      envelope("motor_feedback", 999, { role: "x" });

    const { result } = renderHook(() =>
      useWebTransport("https://x/wt", { decode }),
    );
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });

    const onGap = vi.fn();
    act(() => {
      result.current.onGap(onGap);
    });

    StubWebTransport.last!.datagramReader.push(new Uint8Array([0]));
    await new Promise((r) => setTimeout(r, 30));

    expect(onGap).not.toHaveBeenCalled();
  });

  it("reads length-prefixed envelopes off a reliable uni-stream", async () => {
    // Wire two envelopes into a single stream as `u32 BE length | bytes`.
    // Bytes are opaque to the decoder; we feed the same stub decoder.
    const calls: Uint8Array[] = [];
    let cursor = 0;
    const fixtures = [
      envelope("fault", 0, { code: "OVERTEMP" }),
      envelope("fault", 1, { code: "OVERCURRENT" }),
    ];
    const decode = (b: Uint8Array): unknown => {
      calls.push(b);
      return fixtures[cursor++];
    };

    const { result } = renderHook(() =>
      useWebTransport("https://x/wt", { decode }),
    );
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });

    const onFault = vi.fn();
    act(() => {
      result.current.onKind<{ code: string }>("fault", onFault);
    });

    // Build the wire: [u32 len=3][0xA0,0xA1,0xA2][u32 len=2][0xB0,0xB1].
    // The actual bytes don't matter (decoder is stubbed); the reader
    // just needs to slice them correctly so we cover the framing logic.
    const frame1 = new Uint8Array([0, 0, 0, 3, 0xa0, 0xa1, 0xa2]);
    const frame2 = new Uint8Array([0, 0, 0, 2, 0xb0, 0xb1]);
    const wire = new Uint8Array(frame1.byteLength + frame2.byteLength);
    wire.set(frame1, 0);
    wire.set(frame2, frame1.byteLength);

    // Hand-roll a ReadableStream that emits the wire in one chunk then closes.
    let pushed = false;
    const incomingStream = new ReadableStream<Uint8Array>({
      pull(controller) {
        if (!pushed) {
          controller.enqueue(wire);
          pushed = true;
        } else {
          controller.close();
        }
      },
    });
    StubWebTransport.last!.streamReader.push(incomingStream);
    await new Promise((r) => setTimeout(r, 50));

    expect(calls).toHaveLength(2);
    expect(calls[0]).toHaveLength(3);
    expect(calls[1]).toHaveLength(2);
    expect(onFault).toHaveBeenCalledTimes(2);
    expect(onFault.mock.calls[0][0].data.code).toBe("OVERTEMP");
    expect(onFault.mock.calls[1][0].data.code).toBe("OVERCURRENT");
  });

  it("reassembles a length-prefixed frame split across multiple chunks", async () => {
    const decode = (_: Uint8Array): unknown =>
      envelope("fault", 0, { code: "X" });

    const { result } = renderHook(() =>
      useWebTransport("https://x/wt", { decode }),
    );
    await waitFor(() => {
      expect(result.current.status.connected).toBe(true);
    });

    const onFault = vi.fn();
    act(() => {
      result.current.onKind("fault", onFault);
    });

    // One 5-byte frame split into three chunks: header partial, header
    // complete + body partial, body complete. Exercises the buffered-read
    // path inside readLengthPrefixedFrames.
    const chunks = [
      new Uint8Array([0, 0]),
      new Uint8Array([0, 5, 0xaa]),
      new Uint8Array([0xbb, 0xcc, 0xdd, 0xee]),
    ];
    let i = 0;
    const incoming = new ReadableStream<Uint8Array>({
      pull(controller) {
        if (i < chunks.length) {
          controller.enqueue(chunks[i++]);
        } else {
          controller.close();
        }
      },
    });
    StubWebTransport.last!.streamReader.push(incoming);
    await new Promise((r) => setTimeout(r, 50));

    expect(onFault).toHaveBeenCalledTimes(1);
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
