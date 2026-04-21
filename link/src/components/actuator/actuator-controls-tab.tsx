// Controls tab: enable / stop / save buttons + the dedicated
// Commissioning card (the supported flash-persistent zero workflow) +
// a hold-to-jog dead-man widget.
//
// The legacy "Set zero" button has been moved INTO the Commissioning
// card's "Advanced" disclosure. It still exists (operators occasionally
// want a one-shot RAM re-zero for a measurement they don't want to
// commit to flash), but it's no longer a sibling of Enable/Stop —
// surfacing it there made it look like a normal control, and a
// commissioned motor that gets a stray click on the old button silently
// shifted its frame. The new layout makes "the persistent thing" the
// big button and the diagnostic thing a deliberately-hidden affordance.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { queryKeys } from "@/api";
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
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useLimbHealth } from "@/lib/hooks/useLimbHealth";
import { DeadManJog } from "./dead-man-jog";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import type { Motor } from "@/lib/types/Motor";
import { RAD_TO_DEG } from "@/lib/units";

type Action = "enable" | "stop" | "save";

export function ActuatorControlsTab({ motor }: { motor: MotorSummary }) {
  const qc = useQueryClient();
  const [confirm, setConfirm] = useState<Action | null>(null);
  const limb = useLimbHealth(motor.role);
  const enableTip =
    !motor.verified
      ? "Enable requires a verified motor (Inventory tab)."
      : !limb.healthy && limb.blockReason
        ? limb.blockReason
        : "";

  const mutate = useMutation({
    mutationFn: async (action: Action) => {
      switch (action) {
        case "enable":
          return api.enable(motor.role);
        case "stop":
          return api.stop(motor.role);
        case "save":
          return api.saveToFlash(motor.role);
      }
    },
    onSuccess: () => {
      setConfirm(null);
      qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
      qc.invalidateQueries({ queryKey: queryKeys.params.byRole(motor.role) });
    },
  });

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Motion control</CardTitle>
          <CardDescription>
            Enable arms the controller and starts accepting setpoints. Stop
            issues a type-4 (RS03 motor stop). Save flushes every RAM-resident
            parameter to firmware flash. To re-anchor the joint's mechanical
            zero, use the Commissioning card below.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          {enableTip ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <span className="inline-flex">
                  <Button
                    variant="default"
                    disabled={
                      mutate.isPending || !motor.verified || !limb.healthy
                    }
                    onClick={() => setConfirm("enable")}
                  >
                    Enable
                  </Button>
                </span>
              </TooltipTrigger>
              <TooltipContent className="max-w-xs whitespace-normal">
                {enableTip}
              </TooltipContent>
            </Tooltip>
          ) : (
            <Button
              variant="default"
              disabled={
                mutate.isPending || !motor.verified || !limb.healthy
              }
              onClick={() => setConfirm("enable")}
            >
              Enable
            </Button>
          )}
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
            onClick={() => setConfirm("save")}
          >
            Save to flash
          </Button>
          {!limb.healthy && motor.verified && (
            <p className="w-full text-xs text-amber-400">{limb.blockReason}</p>
          )}
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

      <CommissioningCard motor={motor} />

      <DeadManJog motor={motor} />

      {confirm && (
        <ConfirmDialog
          title={
            {
              enable: "Enable motor",
              stop: "Stop motor",
              save: "Save parameters to flash",
            }[confirm]
          }
          description={describe(confirm, motor.role)}
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

// ─── Commissioning card ────────────────────────────────────────────────────
//
// The big-button entry to the supported flash-persistent zeroing flow:
// `POST /api/motors/:role/commission` runs type-6 SetZero + type-22
// SaveParams + a readback of `add_offset` over CAN, then writes the
// readback value into `inventory.yaml` as `commissioned_zero_offset`.
// On every subsequent boot the orchestrator (Phase C) reads `add_offset`
// back over CAN and refuses motion if it doesn't match — that's why
// commissioning is the supported persistence path and not raw setZero.
//
// The card also surfaces the current commissioned offset (read from
// `inventory.yaml` via the existing GET `:role/inventory` endpoint) so
// the operator can see whether the motor has ever been commissioned and
// what the stored value is. After a successful commission we also show
// the firmware-confirmed readback so the operator has a "yes the
// firmware really did flash this value" feedback loop that the bench
// flow never had.
//
// The legacy raw setZero is parked in a native HTML <details>
// disclosure underneath. It still works — and the daemon now requires
// `confirm_advanced: true` (Phase A.2) so a misclick on the disclosure
// itself can't fire it — but the layout makes the dangerous thing the
// hidden thing.

function CommissioningCard({ motor }: { motor: MotorSummary }) {
  const qc = useQueryClient();
  const [confirm, setConfirm] = useState<"commission" | "set_zero" | null>(
    null,
  );

  // The current commissioned offset / commissioned_at live on the
  // typed `Motor` shape but aren't on `MotorSummary` (yet — Phase F.1
  // may surface them there). Pull them from the inventory passthrough,
  // which round-trips the typed Motor via serde_json::to_value(motor)
  // so the typed scalars are present alongside the YAML extras.
  const inv = useQuery({
    queryKey: queryKeys.inventory.byRole(motor.role),
    queryFn: () => api.getInventory(motor.role),
    retry: false,
  });
  const invMotor = inv.data as (Motor & Record<string, unknown>) | undefined;
  const commissionedOffset =
    typeof invMotor?.commissioned_zero_offset === "number"
      ? invMotor.commissioned_zero_offset
      : null;
  const commissionedAt =
    typeof invMotor?.commissioned_at === "string"
      ? invMotor.commissioned_at
      : null;
  const isCommissioned = commissionedOffset !== null;

  const commission = useMutation({
    mutationFn: () => api.commissionMotor(motor.role),
    onSuccess: () => {
      setConfirm(null);
      qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
      qc.invalidateQueries({ queryKey: queryKeys.inventory.byRole(motor.role) });
      qc.invalidateQueries({ queryKey: queryKeys.params.byRole(motor.role) });
    },
  });

  const setZero = useMutation({
    mutationFn: () => api.setZero(motor.role),
    onSuccess: () => {
      setConfirm(null);
      qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
      qc.invalidateQueries({ queryKey: queryKeys.params.byRole(motor.role) });
    },
  });

  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Commissioning</CardTitle>
          <CardDescription>
            Anchor this joint's mechanical zero at its current physical
            position and persist the new zero to firmware flash. Run this
            once per actuator after physically positioning the joint at its
            desired neutral pose. After commissioning, every boot will
            verify the firmware's stored zero matches the one recorded
            here, and (Phase&nbsp;C) auto-home this actuator to its
            predefined home pose.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap items-center gap-3">
            <Button
              variant="default"
              disabled={commission.isPending || !motor.present}
              onClick={() => setConfirm("commission")}
            >
              {commission.isPending
                ? "Commissioning..."
                : isCommissioned
                  ? "Re-commission zero"
                  : "Commission zero (saves to flash)"}
            </Button>
            <CommissionStatus
              isCommissioned={isCommissioned}
              commissionedOffset={commissionedOffset}
              commissionedAt={commissionedAt}
            />
          </div>

          {!motor.present && (
            <p className="text-xs text-amber-400">
              Motor is marked <code className="font-mono">present: false</code>{" "}
              in inventory.yaml; commissioning requires the motor to be on
              the bus.
            </p>
          )}
          {commission.isError && (
            <CommissionErrorBanner error={commission.error as ApiError} />
          )}
          {commission.isSuccess && (
            <p className="text-xs text-emerald-400">
              Firmware confirmed{" "}
              <code className="font-mono">add_offset</code> ={" "}
              <strong>{formatRad(commission.data.offset_rad)}</strong>; saved
              to flash and recorded in inventory.yaml. Power-cycle the motor
              to confirm the boot orchestrator picks it up.
            </p>
          )}

          <details className="group rounded-md border border-border/60 bg-muted/30 p-3 text-sm">
            <summary className="cursor-pointer select-none font-medium text-muted-foreground hover:text-foreground">
              Advanced: RAM-only set zero
            </summary>
            <div className="mt-3 space-y-2 text-xs text-muted-foreground">
              <p>
                Issues only the type-6 frame (no type-22 save, no inventory
                update, no boot-orchestrator handshake). The new zero lives
                in firmware RAM until the next power cycle, after which the
                previously-saved <code className="font-mono">add_offset</code>{" "}
                takes effect again. Use this only for a measurement you
                don't want to persist; for everything else, use Commission
                zero above.
              </p>
              <Button
                variant="outline"
                size="sm"
                disabled={setZero.isPending || !motor.present}
                onClick={() => setConfirm("set_zero")}
              >
                {setZero.isPending ? "Setting..." : "Set zero (RAM only)"}
              </Button>
              {setZero.isError && (
                <p className="text-destructive">
                  {(setZero.error as ApiError).message}
                </p>
              )}
              {setZero.isSuccess && (
                <p className="text-emerald-400">
                  Zero re-anchored in RAM. Will revert at next power cycle.
                </p>
              )}
            </div>
          </details>
        </CardContent>
      </Card>

      {confirm === "commission" && (
        <ConfirmDialog
          title="Commission zero (writes to flash)"
          description={
            <>
              <p>
                You are about to permanently re-anchor{" "}
                <code className="font-mono">{motor.role}</code>'s mechanical
                zero. The shaft must be at rest at the position you want to
                become the new neutral.
              </p>
              <p className="mt-2">This will:</p>
              <ol className="ml-5 mt-1 list-decimal space-y-1">
                <li>
                  Tell the firmware that the joint's CURRENT physical position
                  is the new zero (type-6 SetZero).
                </li>
                <li>
                  Save that zero to firmware flash so it persists across
                  power cycles (type-22 SaveParams).
                </li>
                <li>
                  Read <code className="font-mono">add_offset</code> back over
                  CAN to confirm the firmware accepted the change.
                </li>
                <li>
                  Record the readback in{" "}
                  <code className="font-mono">inventory.yaml</code> as{" "}
                  <code className="font-mono">commissioned_zero_offset</code>.
                </li>
              </ol>
              <p className="mt-2 text-xs">
                After this, every boot will verify the firmware's stored
                zero matches the recorded value, and the daemon will refuse
                motion if they disagree.
              </p>
              {isCommissioned && (
                <p className="mt-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-2 text-xs text-amber-400">
                  This motor is already commissioned (current{" "}
                  <code className="font-mono">commissioned_zero_offset</code>{" "}
                  = {formatRad(commissionedOffset!)}). Re-commissioning
                  will overwrite that value.
                </p>
              )}
            </>
          }
          confirmLabel="Commission"
          confirmVariant="destructive"
          onCancel={() => setConfirm(null)}
          onConfirm={() => commission.mutate()}
        />
      )}
      {confirm === "set_zero" && (
        <ConfirmDialog
          title="Set mechanical zero (RAM only — does not persist)"
          description={
            <>
              <p>
                Issue a type-6 set-mechanical-zero to{" "}
                <code className="font-mono">{motor.role}</code>. The shaft
                must be at rest.
              </p>
              <p className="mt-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-2 text-xs text-amber-400">
                <strong>This is the RAM-only diagnostic path.</strong> The new
                zero is lost on the next power cycle, and inventory.yaml is
                NOT updated. For a flash-persistent zero, cancel this and use
                the Commission zero button above.
              </p>
            </>
          }
          confirmLabel="Set zero (RAM only)"
          confirmVariant="destructive"
          onCancel={() => setConfirm(null)}
          onConfirm={() => setZero.mutate()}
        />
      )}
    </>
  );
}

function CommissionStatus({
  isCommissioned,
  commissionedOffset,
  commissionedAt,
}: {
  isCommissioned: boolean;
  commissionedOffset: number | null;
  commissionedAt: string | null;
}) {
  if (!isCommissioned) {
    return (
      <span className="text-xs text-amber-400">Not commissioned</span>
    );
  }
  return (
    <span className="text-xs text-muted-foreground">
      Commissioned at{" "}
      <code className="font-mono">{formatRad(commissionedOffset!)}</code>
      {commissionedAt && (
        <>
          {" "}
          on{" "}
          <time dateTime={commissionedAt}>
            {new Date(commissionedAt).toLocaleString()}
          </time>
        </>
      )}
    </span>
  );
}

// Render the commission_failed envelope nicely. The daemon returns
// `{ error: "commission_failed", detail: "step N (...)", readback_rad }`
// rather than the generic ApiError shape, so we pull the readback if
// the failure was at the readback step itself.
function CommissionErrorBanner({ error }: { error: ApiError }) {
  const body = error.body as
    | { error?: string; detail?: string; readback_rad?: number | null }
    | undefined;
  const detail = body?.detail ?? error.message;
  const readback = body?.readback_rad;
  return (
    <div className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive">
      <p>
        <strong>Commission failed:</strong> {detail}
      </p>
      {typeof readback === "number" && (
        <p className="mt-1">
          Firmware reported{" "}
          <code className="font-mono">add_offset</code> ={" "}
          {formatRad(readback)}.
        </p>
      )}
    </div>
  );
}

function formatRad(value: number): string {
  return `${(value * RAD_TO_DEG).toFixed(2)}° (${value.toFixed(4)} rad)`;
}
