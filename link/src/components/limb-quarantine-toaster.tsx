import { useEffect, useState } from "react";
import { Link } from "@tanstack/react-router";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  type LimbQuarantineEvent,
  subscribeLimbQuarantine,
} from "@/lib/limbQuarantineBus";
import { Button } from "@/components/ui/button";

const AUTO_DISMISS_MS = 14_000;

/**
 * Subscribes to limb-quarantine events dispatched from `apiFetch` and shows
 * a single stacked banner with a deep link to the first failing actuator.
 */
export function LimbQuarantineToaster() {
  const [open, setOpen] = useState<LimbQuarantineEvent | null>(null);

  useEffect(() => subscribeLimbQuarantine((ev) => setOpen(ev)), []);

  useEffect(() => {
    if (!open) return;
    const id = window.setTimeout(() => setOpen(null), AUTO_DISMISS_MS);
    return () => window.clearTimeout(id);
  }, [open]);

  if (!open) return null;

  const first = open.failedMotors[0]?.role;
  const names = open.failedMotors.map((m) => `${m.role} (${m.state_kind})`).join(", ");

  return (
    <div
      role="status"
      className={cn(
        "fixed bottom-4 right-4 z-50 max-w-md rounded-lg border border-destructive/50 bg-destructive/15 px-4 py-3 shadow-lg backdrop-blur",
      )}
    >
      <div className="flex gap-3">
        <div className="min-w-0 flex-1 text-sm">
          <p className="font-medium text-destructive">Motion refused — limb quarantined</p>
          <p className="mt-1 text-xs text-muted-foreground">
            {open.limb && (
              <>
                Limb <code className="font-mono text-foreground">{open.limb}</code>
                {": "}
              </>
            )}
            {names || "see detail below"}
          </p>
          {open.detail && (
            <p className="mt-2 text-xs text-foreground/90">{open.detail}</p>
          )}
          {first && (
            <p className="mt-2">
              <Link
                to="/actuators/$role"
                params={{ role: first }}
                className="text-xs font-medium text-primary underline-offset-4 hover:underline"
              >
                Open {first} →
              </Link>
            </p>
          )}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-8 w-8 shrink-0 text-muted-foreground hover:text-foreground"
          aria-label="Dismiss"
          onClick={() => setOpen(null)}
        >
          <X className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}
