// Tests for the WebTransport -> TanStack Query bridge.
//
// The bridge is the only consumer that should be opening a WebTransport
// session in the SPA. These tests pin:
//   1. Live MotorFeedback envelopes merge into the cached MotorSummary[]
//      by role, preserving inventory metadata that doesn't stream.
//   2. Multiple frames within the same animation frame coalesce into a
//      single cache write — this is the whole point of the bridge.
//   3. Frames received before the inventory has been seeded are dropped.
//   4. SystemSnapshot envelopes flow into ['system'].
//   5. Custom reducers via the `reducers` prop work end-to-end (proves
//      the registry is generic and not hard-coded to motor/system).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  WebTransportBridge,
  type WtReducer,
} from "@/lib/hooks/WebTransportBridge";
import { WT_PROTOCOL_VERSION } from "@/lib/hooks/useWebTransport";
import type { MotorFeedback } from "@/lib/types/MotorFeedback";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import type { SystemSnapshot } from "@/lib/types/SystemSnapshot";

// Mock the cbor decoder out: the bridge uses the hook's default decoder,
// which calls cbor-x/decode. We bypass it entirely by intercepting the
// module.
const decodeMock = vi.fn<(b: Uint8Array) => unknown>();
vi.mock("cbor-x/decode", () => ({
  decode: (b: Uint8Array) => decodeMock(b),
}));

// ---------- stub WebTransport plumbing ----------

class StubReader<T> {
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
    if (this.closed) return Promise.resolve({ value: undefined, done: true });
    return new Promise((r) => {
      this.resolveNext = r;
    });
  }
  releaseLock() {}
}

class StubWebTransport {
  static last: StubWebTransport | null = null;
  ready = Promise.resolve();
  reader = new StubReader<Uint8Array>();
  streams = new StubReader<ReadableStream<Uint8Array>>();
  datagrams = { readable: { getReader: () => this.reader } };
  incomingUnidirectionalStreams = { getReader: () => this.streams };
  constructor(public url: string) {
    StubWebTransport.last = this;
  }
  close() {
    this.reader.close();
    this.streams.close();
  }
}

// Build a fully-formed envelope for the decoder mock to return.
function env<T>(kind: string, seq: number, data: T) {
  return {
    v: WT_PROTOCOL_VERSION,
    kind,
    seq,
    t_ms: Date.now(),
    data,
  };
}

function makeFeedback(role: string, t_ms: number, pos: number): MotorFeedback {
  return {
    t_ms: BigInt(t_ms) as unknown as bigint,
    role,
    can_id: 0,
    mech_pos_rad: pos,
    mech_vel_rad_s: 0,
    torque_nm: 0,
    vbus_v: 48,
    temp_c: 32,
    fault_sta: 0,
    warn_sta: 0,
  };
}

/** Full `MotorSummary` for query seeding; keeps required fields in sync with ts-rs output. */
function motorSummaryFixture(
  patch: Pick<
    MotorSummary,
    "role" | "can_bus" | "can_id" | "firmware_version" | "verified" | "present"
  >,
): MotorSummary {
  return {
    enabled: false,
    travel_limits: null,
    predefined_home_rad: null,
    latest: null,
    boot_state: { kind: "unknown" },
    limb: null,
    joint_kind: null,
    drifted_param_count: 0,
    ...patch,
  };
}

function makeSystem(t_ms: number): SystemSnapshot {
  return {
    t_ms: BigInt(t_ms) as unknown as bigint,
    cpu_pct: 17.5,
    load: [0.4, 0.5, 0.6],
    mem_used_mb: 1850n as unknown as bigint,
    mem_total_mb: 8192n as unknown as bigint,
    temps_c: { cpu: 48, gpu: 45 },
    throttled: { now: false, ever: false, raw_hex: "0x0" },
    uptime_s: 1234n as unknown as bigint,
    hostname: "test",
    kernel: "test",
    is_mock: true,
  };
}

let qc: QueryClient;

beforeEach(() => {
  qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  StubWebTransport.last = null;
  decodeMock.mockReset();
  (globalThis as unknown as { WebTransport: typeof StubWebTransport }).WebTransport =
    StubWebTransport;
});

afterEach(() => {
  delete (globalThis as { WebTransport?: unknown }).WebTransport;
  vi.restoreAllMocks();
  qc.clear();
});

function renderBridge(
  url: string | null = "https://x/wt",
  reducers?: WtReducer[],
) {
  return render(
    <QueryClientProvider client={qc}>
      <WebTransportBridge urlOverride={url} reducers={reducers} />
    </QueryClientProvider>,
  );
}

describe("WebTransportBridge", () => {
  it("merges MotorFeedback into the ['motors'] cache by role, preserving metadata", async () => {
    const baseline: MotorSummary[] = [
      motorSummaryFixture({
        role: "shoulder_a",
        can_bus: "can1",
        can_id: 8,
        firmware_version: "1.2.3",
        verified: true,
        present: true,
      }),
      motorSummaryFixture({
        role: "shoulder_b",
        can_bus: "can1",
        can_id: 9,
        firmware_version: "1.2.3",
        verified: false,
        present: true,
      }),
    ];
    qc.setQueryData(["motors"], baseline);

    decodeMock.mockReturnValueOnce(
      env("motor_feedback", 0, makeFeedback("shoulder_a", 1000, 0.42)),
    );

    renderBridge();
    await waitFor(() => {
      expect(StubWebTransport.last).not.toBeNull();
    });
    // Bridge subscribes inside a useEffect; let it commit before pushing.
    await new Promise((r) => setTimeout(r, 30));
    act(() => {
      StubWebTransport.last!.reader.push(new Uint8Array([0]));
    });
    await waitFor(
      () => {
        const cached = qc.getQueryData<MotorSummary[]>(["motors"]);
        expect(cached?.find((m) => m.role === "shoulder_a")?.latest).not.toBeNull();
      },
      { timeout: 1000 },
    );

    const cached = qc.getQueryData<MotorSummary[]>(["motors"])!;
    const a = cached.find((m) => m.role === "shoulder_a")!;
    expect(a.latest?.mech_pos_rad).toBe(0.42);
    expect(a.can_id).toBe(8);
    expect(a.verified).toBe(true);
    expect(a.firmware_version).toBe("1.2.3");
    expect(cached.find((m) => m.role === "shoulder_b")!.latest).toBeNull();
  });

  it("coalesces multiple frames within one animation frame into a single cache write", async () => {
    const baseline: MotorSummary[] = [
      motorSummaryFixture({
        role: "x",
        can_bus: "can1",
        can_id: 1,
        firmware_version: null,
        verified: true,
        present: true,
      }),
    ];
    qc.setQueryData(["motors"], baseline);

    decodeMock
      .mockReturnValueOnce(env("motor_feedback", 0, makeFeedback("x", 1, 0.1)))
      .mockReturnValueOnce(env("motor_feedback", 1, makeFeedback("x", 2, 0.2)))
      .mockReturnValueOnce(env("motor_feedback", 2, makeFeedback("x", 3, 0.3)));

    let writes = 0;
    qc.getQueryCache().subscribe((event) => {
      if (event.type === "updated" && event.action.type === "setState") {
        writes++;
      }
    });

    renderBridge();
    await waitFor(() => {
      expect(StubWebTransport.last).not.toBeNull();
    });
    await new Promise((r) => setTimeout(r, 30));
    act(() => {
      StubWebTransport.last!.reader.push(new Uint8Array([0]));
      StubWebTransport.last!.reader.push(new Uint8Array([0]));
      StubWebTransport.last!.reader.push(new Uint8Array([0]));
    });
    await new Promise((r) => setTimeout(r, 60));

    const cached = qc.getQueryData<MotorSummary[]>(["motors"])!;
    expect(cached[0].latest?.mech_pos_rad).toBe(0.3);
    expect(writes).toBeLessThan(3);
  });

  it("drops MotorFeedback when the ['motors'] cache is empty (no inventory baseline)", async () => {
    decodeMock.mockReturnValueOnce(
      env("motor_feedback", 0, makeFeedback("nobody", 1, 0.5)),
    );

    renderBridge();
    await waitFor(() => {
      expect(StubWebTransport.last).not.toBeNull();
    });
    await new Promise((r) => setTimeout(r, 30));
    act(() => {
      StubWebTransport.last!.reader.push(new Uint8Array([0]));
    });
    await new Promise((r) => setTimeout(r, 60));

    expect(qc.getQueryData(["motors"])).toBeUndefined();
  });

  it("writes SystemSnapshot envelopes into the ['system'] cache", async () => {
    decodeMock.mockReturnValueOnce(
      env("system_snapshot", 0, makeSystem(1000)),
    );

    renderBridge();
    await waitFor(() => {
      expect(StubWebTransport.last).not.toBeNull();
    });
    await new Promise((r) => setTimeout(r, 30));
    act(() => {
      StubWebTransport.last!.reader.push(new Uint8Array([0]));
    });
    await new Promise((r) => setTimeout(r, 60));

    const snap = qc.getQueryData<SystemSnapshot>(["system"])!;
    expect(snap.cpu_pct).toBe(17.5);
    expect(snap.hostname).toBe("test");
  });

  it("never opens a WebTransport session when urlOverride is null", async () => {
    renderBridge(null);
    await new Promise((r) => setTimeout(r, 60));
    expect(StubWebTransport.last).toBeNull();
  });

  it("supports custom reducers (proves the registry is generic)", async () => {
    // A bespoke "fault counter" stream: the reducer counts envelopes and
    // writes the running total into a custom query key. This proves the
    // bridge has zero hard-coded knowledge of the kind universe.
    interface FaultBucket {
      count: number;
    }
    const faultReducer: WtReducer<{ code: string }, FaultBucket> = {
      kind: "fault",
      initBucket: () => ({ count: 0 }),
      merge(bucket) {
        bucket.count += 1;
        return true;
      },
      flush(bucket, queryClient) {
        queryClient.setQueryData<number>(["fault-count"], (prev) => {
          return (prev ?? 0) + bucket.count;
        });
      },
    };

    decodeMock
      .mockReturnValueOnce(env("fault", 0, { code: "A" }))
      .mockReturnValueOnce(env("fault", 1, { code: "B" }))
      .mockReturnValueOnce(env("fault", 2, { code: "C" }));

    renderBridge("https://x/wt", [faultReducer as unknown as WtReducer]);
    await waitFor(() => {
      expect(StubWebTransport.last).not.toBeNull();
    });
    await new Promise((r) => setTimeout(r, 30));
    act(() => {
      StubWebTransport.last!.reader.push(new Uint8Array([0]));
      StubWebTransport.last!.reader.push(new Uint8Array([0]));
      StubWebTransport.last!.reader.push(new Uint8Array([0]));
    });
    await waitFor(
      () => {
        expect(qc.getQueryData<number>(["fault-count"])).toBe(3);
      },
      { timeout: 1000 },
    );
  });

  it("dispatches frames of unknown kinds harmlessly (no listener, no crash)", async () => {
    decodeMock.mockReturnValueOnce(env("totally_unregistered", 0, {}));

    // Empty reducer set so any kind is "unknown".
    renderBridge("https://x/wt", []);
    await waitFor(() => {
      expect(StubWebTransport.last).not.toBeNull();
    });
    await new Promise((r) => setTimeout(r, 30));
    act(() => {
      StubWebTransport.last!.reader.push(new Uint8Array([0]));
    });
    await new Promise((r) => setTimeout(r, 60));

    expect(qc.getQueryData(["motors"])).toBeUndefined();
    expect(qc.getQueryData(["system"])).toBeUndefined();
  });
});
