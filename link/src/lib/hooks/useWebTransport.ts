// WebTransport client hook.
//
// Opens a WebTransport session to the URL advertised by `GET /api/config`
// (falls back to disabled + returns an empty stream when the server reports
// `webtransport.enabled = false`).
//
// Messages arrive as CBOR datagrams; we ship a minimal decoder rather than
// pulling in a full CBOR library for the first pass. This handles only the
// shapes rudydae emits from `types::MotorFeedback`.

import { useEffect, useRef, useState } from "react";
import type { MotorFeedback } from "@/lib/types/MotorFeedback";

export interface WtStatus {
  enabled: boolean;
  connected: boolean;
  error: string | null;
}

type Listener = (fb: MotorFeedback) => void;

export function useWebTransport(url: string | null | undefined): {
  status: WtStatus;
  subscribe: (listener: Listener) => () => void;
} {
  const [status, setStatus] = useState<WtStatus>({
    enabled: !!url,
    connected: false,
    error: null,
  });
  const listenersRef = useRef<Set<Listener>>(new Set());

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
        setStatus({ enabled: true, connected: true, error: null });

        const reader = transport.datagrams.readable.getReader();
        try {
          while (!cancelled) {
            const { value, done } = await reader.read();
            if (done || !value) break;
            const fb = decodeFeedback(value);
            if (fb) {
              for (const l of listenersRef.current) l(fb);
            }
          }
        } finally {
          reader.releaseLock();
        }
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
  };
}

// Intentionally left for Phase 2: a proper CBOR decoder (via `cbor-x` or
// hand-rolled) producing MotorFeedback. Until real hardware + TLS are wired,
// the UI falls back to the REST `GET /api/motors/:role/feedback` poll, so
// this helper is a stub.
function decodeFeedback(_bytes: Uint8Array): MotorFeedback | null {
  return null;
}
