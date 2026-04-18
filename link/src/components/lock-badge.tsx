// Single-operator lock badge.
//
// Shows the current holder + a button that acquires (when free), takes
// over (when held by someone else), or releases (when held by us). The
// daemon fans `safety_event` `lock_changed` frames over WebTransport so
// every other tab updates without polling — we re-fetch the `["lock"]`
// query whenever a `safety_event` frame lands.

import { useEffect } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Lock, LockOpen } from "lucide-react";
import { api } from "@/lib/api";
import { sessionId } from "@/lib/session";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { getBridgeWt } from "@/lib/hooks/wt-bridge-handle";
import { useWtConnected } from "@/lib/hooks/wt-status";

export function LockBadge({ className }: { className?: string }) {
  const qc = useQueryClient();
  const wtConnected = useWtConnected();
  const lockQ = useQuery({
    queryKey: ["lock"],
    queryFn: () => api.lock.get(),
    // Fall back to a 5s poll when WT is down; 30s safety net otherwise.
    refetchInterval: wtConnected ? 30_000 : 5_000,
  });

  // Live invalidation on safety_event(lock_changed).
  useEffect(() => {
    const wt = getBridgeWt();
    if (!wt) return;
    return wt.onKind<{ kind: string }>("safety_event", (env) => {
      if (env.data.kind === "lock_changed") {
        qc.invalidateQueries({ queryKey: ["lock"] });
      }
    });
  }, [qc]);

  const acquire = useMutation({
    mutationFn: () => api.lock.acquire(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["lock"] }),
  });
  const release = useMutation({
    mutationFn: () => api.lock.release(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["lock"] }),
  });

  const lock = lockQ.data;
  const me = sessionId();
  const free = !lock?.holder;
  const youHold = !!lock?.you_hold;
  const holderShort = lock?.holder ? lock.holder.slice(0, 8) : null;

  return (
    <div className={`flex items-center gap-2 ${className ?? ""}`}>
      <Badge
        variant={free ? "outline" : youHold ? "success" : "warning"}
        className="gap-1"
      >
        {free ? (
          <LockOpen className="h-3 w-3" />
        ) : (
          <Lock className="h-3 w-3" />
        )}
        {free
          ? "lock free"
          : youHold
            ? "you have control"
            : `held by ${holderShort}`}
      </Badge>
      {free && (
        <Button
          size="sm"
          variant="default"
          disabled={acquire.isPending}
          onClick={() => acquire.mutate()}
        >
          Take control
        </Button>
      )}
      {!free && !youHold && (
        <Button
          size="sm"
          variant="destructive"
          disabled={acquire.isPending}
          onClick={() => acquire.mutate()}
        >
          Take over
        </Button>
      )}
      {youHold && (
        <Button
          size="sm"
          variant="ghost"
          disabled={release.isPending}
          onClick={() => release.mutate()}
        >
          Release
        </Button>
      )}
      {(acquire.isError || release.isError) && (
        <span className="text-xs text-destructive">
          {((acquire.error || release.error) as Error)?.message}
        </span>
      )}
      <span
        className="hidden text-xs text-muted-foreground md:inline"
        title="Your per-tab session id"
      >
        you: {me.slice(0, 8)}
      </span>
    </div>
  );
}
