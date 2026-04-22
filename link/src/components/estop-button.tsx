// Persistent E-stop button.
//
// Always reachable from anywhere in the SPA — sits in the app shell header.
// A single click dispatches `POST /api/estop` (no confirm step). The daemon
// broadcasts a `safety_event` so other tabs flash a banner.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ShieldAlert } from "lucide-react";
import { queryKeys } from "@/api";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";

export function EstopButton({ className }: { className?: string }) {
  const qc = useQueryClient();
  const fire = useMutation({
    mutationFn: () => api.estop(),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
    },
  });

  return (
    <>
      <Button
        variant="destructive"
        className={`gap-2 ${className ?? ""}`}
        onClick={() => fire.mutate()}
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
    </>
  );
}
