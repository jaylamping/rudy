// One row of the firmware-params table.
//
// Originally inlined inside `_app.params.tsx`; pulled out so the
// per-actuator detail page can reuse the same row + confirm dialog.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { queryKeys } from "@/api";
import { api, ApiError } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { JsonValue } from "@/lib/types/serde_json/JsonValue";
import type { ParamValue } from "@/lib/types/ParamValue";
import {
  degToRad,
  isAngleUnit,
  isAngularVelUnit,
  radToDeg,
} from "@/lib/units";
import { OfflineActionTooltip } from "@/components/actuator/offline-action-tooltip";
import { useDeviceLive, useDeviceOfflineTip } from "@/store";
import { ConfirmDialog } from "./confirm-dialog";

export interface ParamRowProps {
  role: string;
  param: ParamValue;
}

function paramDisplayMode(param: ParamValue): {
  displayInDeg: boolean;
  displayUnit: string;
} {
  const angle = isAngleUnit(param.units);
  const angVel = isAngularVelUnit(param.units);
  if (angle || angVel) {
    return {
      displayInDeg: true,
      displayUnit: angVel && !angle ? "°/s" : "°",
    };
  }
  return { displayInDeg: false, displayUnit: param.units ?? "" };
}

function wireNumericValue(value: JsonValue): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  return null;
}

export function ParamRow({ role, param }: ParamRowProps) {
  const isLive = useDeviceLive(role);
  const offlineTip = useDeviceOfflineTip(role);
  const qc = useQueryClient();
  const { displayInDeg, displayUnit } = paramDisplayMode(param);
  const [draft, setDraft] = useState<string>("");
  const [confirmSave, setConfirmSave] = useState(false);

  useEffect(() => {
    const n = wireNumericValue(param.value);
    if (displayInDeg && n != null) {
      setDraft(String(radToDeg(n)));
    } else {
      setDraft(
        param.value === undefined || param.value === null
          ? ""
          : String(param.value),
      );
    }
  }, [param.value, displayInDeg]);

  const write = useMutation({
    mutationFn: async () => {
      const raw = parseValue(draft, param.type);
      const value: JsonValue =
        displayInDeg && typeof raw === "number"
          ? degToRad(raw)
          : raw;
      return api.writeParam(role, param.name, { value, save_after: true });
    },
    onSuccess: () => {
      setConfirmSave(false);
      qc.invalidateQueries({ queryKey: queryKeys.params.byRole(role) });
      qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
    },
  });

  const adopt = useMutation({
    mutationFn: () => api.adoptParam(role, param.name),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: queryKeys.params.byRole(role) });
      qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
    },
  });

  const push = useMutation({
    mutationFn: () => api.syncParams(role, { names: [param.name] }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: queryKeys.params.byRole(role) });
      qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
    },
  });

  const [lo, hi] = param.hardware_range ?? [null, null];
  const rangeCell =
    lo !== null && hi !== null
      ? displayInDeg
        ? `[${radToDeg(lo).toFixed(4)}, ${radToDeg(hi).toFixed(4)}]`
        : `[${lo}, ${hi}]`
      : "-";

  return (
    <tr className="border-t border-border/60 align-middle">
      <td className="px-3 py-2 font-mono">{param.name}</td>
      <td className="px-3 py-2 font-mono text-muted-foreground">
        0x{param.index.toString(16).toUpperCase().padStart(4, "0")}
      </td>
      <td className="px-3 py-2">
        <input
          className="w-32 rounded-md border border-input bg-background px-2 py-1 font-mono text-sm"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          disabled={!isLive}
        />
      </td>
      <td className="px-3 py-2 font-mono text-muted-foreground">{rangeCell}</td>
      <td className="px-3 py-2 text-muted-foreground">{displayUnit}</td>
      <td className="px-3 py-2">
        <div className="flex flex-wrap items-center gap-2">
          {param.drift && (
            <Badge
              variant="outline"
              className="border-amber-500/60 text-amber-600 dark:text-amber-400"
              title={`live ${JSON.stringify(param.drift.live)} vs desired ${JSON.stringify(param.drift.desired)}`}
            >
              Drift
            </Badge>
          )}
          <OfflineActionTooltip isLive={isLive} offlineTip={offlineTip}>
            <Button
              variant="default"
              size="sm"
              onClick={() => setConfirmSave(true)}
              disabled={write.isPending || !isLive}
            >
              Save
            </Button>
          </OfflineActionTooltip>
          {param.drift && (
            <>
              <OfflineActionTooltip isLive={isLive} offlineTip={offlineTip}>
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => push.mutate()}
                  disabled={push.isPending || !isLive}
                >
                  Push
                </Button>
              </OfflineActionTooltip>
              <OfflineActionTooltip isLive={isLive} offlineTip={offlineTip}>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => adopt.mutate()}
                  disabled={adopt.isPending || !isLive}
                >
                  Adopt
                </Button>
              </OfflineActionTooltip>
            </>
          )}
        </div>
        {(write.isError || adopt.isError || push.isError) && (
          <div className="mt-1 text-xs text-destructive">
            {((write.error ?? adopt.error ?? push.error) as ApiError).message}
          </div>
        )}
        {confirmSave && (
          <ConfirmDialog
            title="Save parameter"
            description={
              <>
                Write <code className="font-mono">{param.name}</code> ={" "}
                <code className="font-mono">{draft}</code> {displayUnit}{" "}
                on <code className="font-mono">{role}</code>, save to actuator
                flash, and record as desired in inventory.
              </>
            }
            confirmLabel="Save"
            confirmVariant="destructive"
            onCancel={() => setConfirmSave(false)}
            onConfirm={() => write.mutate()}
          />
        )}
      </td>
    </tr>
  );
}

function parseValue(s: string, ty: string): JsonValue {
  if (ty.startsWith("u") || ty === "uint8" || ty === "uint16" || ty === "uint32") {
    const n = Number(s);
    if (!Number.isFinite(n)) throw new Error(`expected integer, got ${s}`);
    return Math.trunc(n);
  }
  const n = Number(s);
  if (!Number.isFinite(n)) throw new Error(`expected number, got ${s}`);
  return n;
}
