// Operator-triggered daemon restart.
//
// Sits next to the E-stop in the app shell header so it's always one click
// away. Designed for the post-deploy "did the new build actually take?"
// loop: rather than SSH-ing in to run `systemctl restart cortex`, the
// operator clicks the button, the daemon drops torque on every motor and
// exits, and systemd brings the new binary back up under `Restart=always`.
//
// UX sequencing:
//
//  1. Click → confirm dialog ("Restart cortex?"). Plain "are you sure?"
//     modal, same posture as ConfirmDialog elsewhere — Rudy is a
//     single-operator tailnet console (ADR-0004 D5), the misclick cost is
//     low because we just stop motors and bounce.
//  2. Operator confirms → POST /api/restart. The server drops torque,
//     audit-logs, returns 202 with `restart_in_ms` + `supervised`. The
//     daemon then exits ~500ms later.
//  3. SPA enters a "Restarting…" overlay, polls /api/health every 750ms
//     with a short timeout. As soon as a 2xx (or 503 — anything that
//     parses as JSON, meaning the daemon answered) lands, it forces a
//     full `window.location.reload()` so the browser pulls the freshly
//     deployed JS bundle, not the stale one cached against the previous
//     daemon's etag.
//  4. If the daemon doesn't come back within `RECOVERY_TIMEOUT_MS`, the
//     overlay surfaces a "still down" message with a retry that closes
//     the overlay (the operator can refresh manually or SSH in). That's
//     the only failure mode — we deliberately don't try to be clever
//     about diagnosing *why* the daemon didn't come back.

import { useMutation } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { RefreshCw } from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/params";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

const HEALTH_POLL_INTERVAL_MS = 750;
const HEALTH_PROBE_TIMEOUT_MS = 1500;
const RECOVERY_TIMEOUT_MS = 60_000;

type Phase =
  | { kind: "idle" }
  | { kind: "confirm" }
  | {
      kind: "waiting";
      supervised: boolean;
      stopped: number;
      sinceMs: number;
    }
  | { kind: "recovered"; supervised: boolean }
  | { kind: "timeout"; supervised: boolean };

export function RestartButton({ className }: { className?: string }) {
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });

  const fire = useMutation({
    mutationFn: () => api.restart(),
    onSuccess: (resp) => {
      setPhase({
        kind: "waiting",
        supervised: resp.supervised,
        stopped: resp.stopped,
        sinceMs: Date.now(),
      });
    },
  });

  return (
    <>
      <Button
        variant="outline"
        className={`gap-2 ${className ?? ""}`}
        onClick={() => setPhase({ kind: "confirm" })}
        disabled={fire.isPending || phase.kind === "waiting"}
        title="Restart the cortex daemon"
      >
        <RefreshCw className="h-4 w-4" />
        Restart
      </Button>

      {phase.kind === "confirm" && (
        <ConfirmDialog
          title="Restart cortex daemon"
          description={
            <>
              Drops torque on every present motor (same as E-stop), then
              exits the daemon. Under systemd it will be brought back up
              automatically within a few seconds; in dev (
              <code>npm run dev</code>) you'll need to restart{" "}
              <code>cortex</code> by hand. Useful right after a deploy to
              confirm the new build is live.
            </>
          }
          confirmLabel="Restart now"
          confirmVariant="destructive"
          onCancel={() => setPhase({ kind: "idle" })}
          onConfirm={() => fire.mutate()}
        />
      )}

      {fire.isError && phase.kind !== "waiting" && (
        <span className="text-xs text-destructive">
          {(fire.error as ApiError).message}
        </span>
      )}

      <RecoveryDialog phase={phase} setPhase={setPhase} />
    </>
  );
}

function RecoveryDialog({
  phase,
  setPhase,
}: {
  phase: Phase;
  setPhase: (p: Phase) => void;
}) {
  // Refs so the polling effect can read the latest phase without
  // restarting itself on every state mutation.
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  useEffect(() => {
    if (phase.kind !== "waiting") return;
    let cancelled = false;
    let timer: number | undefined;

    const probe = async (): Promise<boolean> => {
      const controller = new AbortController();
      const timeout = window.setTimeout(
        () => controller.abort(),
        HEALTH_PROBE_TIMEOUT_MS,
      );
      try {
        const res = await fetch("/api/health", {
          method: "GET",
          cache: "no-store",
          signal: controller.signal,
        });
        // Health returns 200 (healthy) or 503 (degraded — e.g. SPA not
        // embedded in dev). Both mean "the daemon is answering," which
        // is all we care about for recovery detection.
        return res.status === 200 || res.status === 503;
      } catch {
        return false;
      } finally {
        window.clearTimeout(timeout);
      }
    };

    const tick = async () => {
      if (cancelled) return;
      const current = phaseRef.current;
      if (current.kind !== "waiting") return;
      if (Date.now() - current.sinceMs > RECOVERY_TIMEOUT_MS) {
        setPhase({ kind: "timeout", supervised: current.supervised });
        return;
      }
      const ok = await probe();
      if (cancelled) return;
      if (ok) {
        setPhase({ kind: "recovered", supervised: current.supervised });
        // Hard reload so any newly-deployed JS bundle is fetched fresh
        // instead of being served from the in-memory router state.
        // Brief delay so the operator sees the "back up" confirmation.
        window.setTimeout(() => window.location.reload(), 500);
        return;
      }
      timer = window.setTimeout(tick, HEALTH_POLL_INTERVAL_MS);
    };

    // First probe runs after the daemon's announced exit delay (~500ms);
    // until then any health hit would be answered by the about-to-die
    // process, falsely declaring victory.
    timer = window.setTimeout(tick, HEALTH_POLL_INTERVAL_MS);

    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [phase.kind, setPhase]);

  if (
    phase.kind !== "waiting" &&
    phase.kind !== "recovered" &&
    phase.kind !== "timeout"
  ) {
    return null;
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        // Only allow dismiss after recovery or timeout; "waiting" must
        // run to completion (or be ignored — the overlay is informational).
        if (open) return;
        if (phase.kind !== "waiting") setPhase({ kind: "idle" });
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {phase.kind === "waiting" && "Restarting cortex…"}
            {phase.kind === "recovered" && "Daemon is back"}
            {phase.kind === "timeout" && "Daemon hasn't come back yet"}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-2 text-sm text-muted-foreground">
          {phase.kind === "waiting" && (
            <>
              <p>
                Stopped {phase.stopped} motor
                {phase.stopped === 1 ? "" : "s"}. Waiting for the daemon to
                come back up…
              </p>
              {!phase.supervised && (
                <p className="text-amber-300/90">
                  No supervisor detected — you may need to restart{" "}
                  <code>cortex</code> by hand. The page will reload
                  automatically once it answers again.
                </p>
              )}
              <div className="flex items-center gap-2 pt-1 text-xs">
                <RefreshCw className="h-3 w-3 animate-spin" />
                polling <code>/api/health</code> every{" "}
                {Math.round(HEALTH_POLL_INTERVAL_MS / 100) / 10}s
              </div>
            </>
          )}
          {phase.kind === "recovered" && (
            <p>Reloading the page to pick up the fresh build…</p>
          )}
          {phase.kind === "timeout" && (
            <>
              <p>
                No response from <code>/api/health</code> after{" "}
                {Math.round(RECOVERY_TIMEOUT_MS / 1000)}s.
              </p>
              {phase.supervised ? (
                <p>
                  systemd may still be retrying — check{" "}
                  <code>journalctl -u cortex -f</code> on the host.
                </p>
              ) : (
                <p>Restart cortex manually, then refresh this page.</p>
              )}
              <div className="pt-2">
                <Button
                  variant="outline"
                  onClick={() => window.location.reload()}
                >
                  Reload page
                </Button>
              </div>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
