// Pin the wire shape the dead-man jog (and any future client→server
// motion surface) writes onto a WebTransport bidi stream.
//
// The daemon is the source of truth for the format; if either side
// changes its mind we want a noisy failure here, not a silent stalled
// motor on staging.

import { describe, expect, it, vi } from "vitest";
import { decode as cborDecode } from "cbor-x/decode";
import { openClientStream } from "@/lib/wt/clientStream";

interface CapturedWrite {
  bytes: Uint8Array;
}

function fakeTransport() {
  const writes: CapturedWrite[] = [];
  let writerClosed = false;
  let writerLocked = false;

  const stream = {
    writable: {
      getWriter: () => {
        writerLocked = true;
        return {
          write: vi.fn(async (b: Uint8Array) => {
            writes.push({ bytes: new Uint8Array(b) });
          }),
          close: vi.fn(async () => {
            writerClosed = true;
          }),
          releaseLock: () => {
            writerLocked = false;
          },
        };
      },
    },
  };

  const transport = {
    createBidirectionalStream: vi.fn(async () => stream),
  } as unknown as WebTransport;

  return {
    transport,
    writes,
    isClosed: () => writerClosed,
    isLocked: () => writerLocked,
  };
}

/** Decode a `u32 BE length | CBOR` frame back into a JS value. */
function decodeFrame(bytes: Uint8Array): unknown {
  expect(bytes.byteLength).toBeGreaterThanOrEqual(4);
  const len =
    (bytes[0] << 24) | (bytes[1] << 16) | (bytes[2] << 8) | bytes[3];
  expect(bytes.byteLength).toBe(4 + len);
  return cborDecode(bytes.subarray(4));
}

describe("openClientStream", () => {
  it("writes length-prefixed CBOR for a MotionJog frame", async () => {
    const { transport, writes } = fakeTransport();
    const handle = await openClientStream(transport);
    await handle.send({
      kind: "motion_jog",
      role: "shoulder_actuator_a",
      vel_rad_s: 0.25,
    });
    await handle.close();

    expect(writes.length).toBeGreaterThanOrEqual(1);
    const decoded = decodeFrame(writes[0].bytes);
    expect(decoded).toMatchObject({
      kind: "motion_jog",
      role: "shoulder_actuator_a",
      vel_rad_s: 0.25,
    });
  });

  it("emits a MotionStop frame when close is called with a role", async () => {
    const { transport, writes, isClosed } = fakeTransport();
    const handle = await openClientStream(transport);
    await handle.close("shoulder_actuator_a");

    // First (and only) frame must be the stop frame.
    expect(writes.length).toBe(1);
    const decoded = decodeFrame(writes[0].bytes);
    expect(decoded).toMatchObject({
      kind: "motion_stop",
      role: "shoulder_actuator_a",
    });
    // Stream is closed end-to-end, mirroring the daemon's
    // ClientGone-on-EOF safety net.
    expect(isClosed()).toBe(true);
    expect(handle.closed).toBe(true);
  });

  it("does not write a stop frame when close is called without a role", async () => {
    const { transport, writes, isClosed } = fakeTransport();
    const handle = await openClientStream(transport);
    await handle.close();
    expect(writes.length).toBe(0);
    expect(isClosed()).toBe(true);
  });

  it("close is idempotent", async () => {
    const { transport, writes } = fakeTransport();
    const handle = await openClientStream(transport);
    await handle.close("role_a");
    await handle.close("role_a");
    // Only one stop frame even though close() was called twice — the
    // operator-side semantics of releasing a held button must not
    // double-stop and re-fire the daemon's audit log.
    expect(writes.length).toBe(1);
  });

  it("preserves write order across many sends", async () => {
    const { transport, writes } = fakeTransport();
    const handle = await openClientStream(transport);
    // Fire three sends without awaiting between them; the helper's
    // internal chain must serialize them on the writer.
    const p1 = handle.send({
      kind: "motion_jog",
      role: "r",
      vel_rad_s: 0.1,
    });
    const p2 = handle.send({
      kind: "motion_jog",
      role: "r",
      vel_rad_s: 0.2,
    });
    const p3 = handle.send({
      kind: "motion_jog",
      role: "r",
      vel_rad_s: 0.3,
    });
    await Promise.all([p1, p2, p3]);
    await handle.close();

    expect(writes.length).toBe(3);
    const vels = writes.map(
      (w) => (decodeFrame(w.bytes) as { vel_rad_s: number }).vel_rad_s,
    );
    expect(vels).toEqual([0.1, 0.2, 0.3]);
  });
});
