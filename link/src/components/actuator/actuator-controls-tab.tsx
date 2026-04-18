// Controls tab: enable / stop / set-zero / save-to-flash buttons, all
// gated by the typed-confirm dialog. Plus a hold-to-jog dead-man widget
// (filled in once `POST /api/motors/:role/jog` lands in the daemon).

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { ConfirmDialog } from "@/components/params";
import { DeadManJog } from "./dead-man-jog";
import type { MotorSummary } from "@/lib/types/MotorSummary";

type Action = "enable" | "stop" | "set_zero" | "save";

export function ActuatorControlsTab({ motor }: { motor: MotorSummary }) {
  const qc = useQueryClient();
  const [confirm, setConfirm] = useState<Action | null>(null);

  const mutate = useMutation({
    mutationFn: async (action: Action) => {
      switch (action) {
        case "enable":
          return api.enable(motor.role);
        case "stop":
          return api.stop(motor.role);
        case "set_zero":
          return api.setZero(motor.role);
        case "save":
          return api.saveToFlash(motor.role);
      }
    },
    onSuccess: () => {
      setConfirm(null);
      qc.invalidateQueries({ queryKey: ["motors"] });
      qc.invalidateQueries({ queryKey: ["params", motor.role] });
    },
  });

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Motion control</CardTitle>
          <CardDescription>
            Enable arms the controller and starts accepting setpoints. Stop
            issues a type-4 (RS03 motor stop). Set zero re-anchors the
            mechanical reference at the current shaft position.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          <Button
            variant="default"
            disabled={mutate.isPending || !motor.verified}
            onClick={() => setConfirm("enable")}
          >
            Enable
          </Button>
          <Button
            variant="destructive"
            disabled={mutate.isPending}
            onClick={() => setConfirm("stop")}
          >
            Stop
          </Button>
          <Button
            variant="outline"
            disabled={mutate.isPending}
            onClick={() => setConfirm("set_zero")}
          >
            Set zero
          </Button>
          <Button
            variant="outline"
            disabled={mutate.isPending}
            onClick={() => setConfirm("save")}
          >
            Save to flash
          </Button>
          {!motor.verified && (
            <p className="w-full text-xs text-amber-400">
              Enable is locked while the motor is unverified. Mark it verified
              from the Inventory tab once commissioning is complete.
            </p>
          )}
          {mutate.isError && (
            <p className="w-full text-xs text-destructive">
              {(mutate.error as ApiError).message}
            </p>
          )}
        </CardContent>
      </Card>

      <DeadManJog motor={motor} />

      {confirm && (
        <ConfirmDialog
          title={
            {
              enable: "Enable motor",
              stop: "Stop motor",
              set_zero: "Set mechanical zero",
              save: "Save parameters to flash",
            }[confirm]
          }
          description={describe(confirm, motor.role)}
          phrase={`${confirm} ${motor.role}`}
          confirmLabel={confirm === "enable" ? "Enable" : "Confirm"}
          confirmVariant={confirm === "enable" ? "default" : "destructive"}
          onCancel={() => setConfirm(null)}
          onConfirm={() => mutate.mutate(confirm)}
        />
      )}
    </div>
  );
}

function describe(action: Action, role: string) {
  switch (action) {
    case "enable":
      return (
        <>
          Issue a type-3 enable to <code className="font-mono">{role}</code>.
          The controller will start tracking setpoints; make sure no humans or
          fragile geometry are inside the workspace.
        </>
      );
    case "stop":
      return (
        <>
          Issue a type-4 motor-stop to <code className="font-mono">{role}</code>
          . The motor disables outputs; any in-flight motion ends at the
          current position with damping per the firmware setting.
        </>
      );
    case "set_zero":
      return (
        <>
          Issue a type-6 set-mechanical-zero to{" "}
          <code className="font-mono">{role}</code>. The shaft must be at rest;
          the new zero offset is RAM-only until you also save to flash.
        </>
      );
    case "save":
      return (
        <>
          Issue a type-22 save-params to <code className="font-mono">{role}</code>
          . Every RAM-resident parameter is persisted to flash and survives
          power cycles.
        </>
      );
  }
}
