// Persistent E-stop button.
//
// Always reachable from anywhere in the SPA — sits in the app shell header.
// Single click pops a confirm dialog and dispatches `POST /api/estop`. The
// daemon broadcasts a `safety_event` so other tabs flash a banner.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { ShieldAlert } from "lucide-react";
import { queryKeys } from "@/api";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/params";

export function EstopButton({ className }: { className?: string }) {
  const qc = useQueryClient();
  const [confirm, setConfirm] = useState(false);
  const fire = useMutation({
    mutationFn: () => api.estop(),
    onSuccess: () => {
      setConfirm(false);
      qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
    },
  });

  return (
    <>
      <Button
        variant="destructive"
        className={`gap-2 ${className ?? ""}`}
        onClick={() => setConfirm(true)}
        disabled={fire.isPending}
      >
        <ShieldAlert className="h-4 w-4" />
        E-STOP
      </Button>
      {fire.isError && (
        <span className="text-xs text-destructive">
          {(fire.error as ApiError).message}
        </span>
      )}
      {confirm && (
        <ConfirmDialog
          title="Global e-stop"
          description={
            <>
              Issue type-4 stop to <strong>every present motor</strong>. This
              cannot be partial — every actuator on every CAN bus will receive
              a stop frame.
            </>
          }
          confirmLabel="STOP NOW"
          confirmVariant="destructive"
          onCancel={() => setConfirm(false)}
          onConfirm={() => fire.mutate()}
        />
      )}
    </>
  );
}
