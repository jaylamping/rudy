// SPA-side helper for the client-to-server WebTransport bidi protocol.
//
// The daemon defines [`crate::wt_client::ClientFrame`] as the wire enum
// for everything the SPA can write into a bidi stream. This file is the
// matching encoder and lifecycle wrapper.
//
// # Wire format
//
// Each frame is `u32 BE length | CBOR body (ClientFrame)`. A bidi stream
// can carry many frames over its lifetime; the daemon dispatches each as
// it arrives. Stream EOF without a preceding `MotionStop` is treated as
// `MotionStopReason::ClientGone` for any motion the stream owned.
//
// # Why a dedicated helper (vs. inlining at the call site)
//
// The dead-man jog needs three things from one stream:
//   1. send a `MotionJog` on pointerDown,
//   2. send a `MotionHeartbeat` every 200 ms while held,
//   3. send a `MotionStop` and close on pointerUp / unmount.
//
// Centralizing the encoder here means a future tele-op surface (joystick,
// retarget, sequencer) can reuse the framer without re-deriving the
// length-prefix details.

import { encode as cborEncode } from "cbor-x/encode";
import type { ClientFrame } from "@/lib/types/ClientFrame";
import { prefixWithLength } from "@/lib/hooks/useWebTransport";

/** Live handle to one open bidi stream. */
export interface ClientStreamHandle {
  /** Send one `ClientFrame`. Resolves once the bytes are queued. */
  send(frame: ClientFrame): Promise<void>;
  /**
   * Send `MotionStop` for `role` (if `role` is provided) and close the
   * stream. Idempotent: subsequent calls resolve immediately.
   */
  close(stopRole?: string): Promise<void>;
  /** True after `close()` has been called. */
  readonly closed: boolean;
}

/**
 * Open a fresh bidi stream on the live WebTransport. The returned handle
 * stays alive until `close()` is invoked or the QUIC session drops.
 *
 * Throws if the platform doesn't support `createBidirectionalStream`
 * (older browsers without WT) so the caller can fall back to REST.
 */
export async function openClientStream(
  transport: WebTransport,
): Promise<ClientStreamHandle> {
  const stream = await transport.createBidirectionalStream();
  const writer = stream.writable.getWriter();
  let closed = false;
  let chain: Promise<unknown> = Promise.resolve();

  const send = async (frame: ClientFrame): Promise<void> => {
    if (closed) return;
    // Serialize writes through a chained promise so a fast caller can't
    // interleave two `write()` calls on the same writer (which would be
    // a runtime error).
    chain = chain.then(() => writer.write(prefixWithLength(cborEncode(frame))));
    await chain;
  };

  const close = async (stopRole?: string): Promise<void> => {
    if (closed) return;
    closed = true;
    try {
      if (stopRole) {
        try {
          await chain.then(() =>
            writer.write(
              prefixWithLength(
                cborEncode({ kind: "motion_stop", role: stopRole }),
              ),
            ),
          );
        } catch {
          // The stop is best-effort: if the writer is already errored
          // the daemon's per-stream task will treat the EOF as
          // `ClientGone` and stop the motion anyway.
        }
      }
      try {
        await writer.close();
      } catch {
        // ignore
      }
    } finally {
      try {
        writer.releaseLock();
      } catch {
        // ignore
      }
    }
  };

  return {
    send,
    close,
    get closed() {
      return closed;
    },
  };
}
