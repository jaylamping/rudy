// One row of the firmware-params table.
//
// Originally inlined inside `_app.params.tsx`; pulled out so the
// per-actuator detail page can reuse the same row + confirm dialog.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, ApiError } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { JsonValue } from "@/lib/types/serde_json/JsonValue";
import type { ParamValue } from "@/lib/types/ParamValue";
import { ConfirmDialog } from "./confirm-dialog";

export interface ParamRowProps {
  role: string;
  param: ParamValue;
}

export function ParamRow({ role, param }: ParamRowProps) {
  const qc = useQueryClient();
  const [draft, setDraft] = useState<string>(String(param.value ?? ""));
  const [confirmSave, setConfirmSave] = useState(false);

  useEffect(() => {
    setDraft(String(param.value ?? ""));
  }, [param.value]);

  const write = useMutation({
    mutationFn: async () => {
      const value = parseValue(draft, param.type);
      return api.writeParam(role, param.name, { value, save_after: true });
    },
    onSuccess: () => {
      setConfirmSave(false);
      qc.invalidateQueries({ queryKey: ["params", role] });
      qc.invalidateQueries({ queryKey: ["motors"] });
    },
  });

  const adopt = useMutation({
    mutationFn: () => api.adoptParam(role, param.name),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["params", role] });
      qc.invalidateQueries({ queryKey: ["motors"] });
    },
  });

  const push = useMutation({
    mutationFn: () => api.syncParams(role, { names: [param.name] }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["params", role] });
      qc.invalidateQueries({ queryKey: ["motors"] });
    },
  });

  const [lo, hi] = param.hardware_range ?? [null, null];

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
        />
      </td>
      <td className="px-3 py-2 font-mono text-muted-foreground">
        {lo !== null && hi !== null ? `[${lo}, ${hi}]` : "-"}
      </td>
      <td className="px-3 py-2 text-muted-foreground">{param.units ?? ""}</td>
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
          <Button
            variant="default"
            size="sm"
            onClick={() => setConfirmSave(true)}
            disabled={write.isPending}
          >
            Save
          </Button>
          {param.drift && (
            <>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => push.mutate()}
                disabled={push.isPending}
              >
                Push
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => adopt.mutate()}
                disabled={adopt.isPending}
              >
                Adopt
              </Button>
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
                <code className="font-mono">{draft}</code> {param.units ?? ""}{" "}
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
