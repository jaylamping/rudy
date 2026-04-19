// One row of the firmware-params table.
//
// Originally inlined inside `_app.params.tsx`; pulled out so the
// per-actuator detail page can reuse the same row + confirm dialog.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, ApiError } from "@/lib/api";
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
  const [confirm, setConfirm] = useState<null | { save: boolean }>(null);

  const write = useMutation({
    mutationFn: async ({ save }: { save: boolean }) => {
      const value = parseValue(draft, param.type);
      return api.writeParam(role, param.name, { value, save_after: save });
    },
    onSuccess: () => {
      setConfirm(null);
      qc.invalidateQueries({ queryKey: ["params", role] });
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
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => setConfirm({ save: false })}
            disabled={write.isPending}
          >
            Write RAM
          </Button>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => setConfirm({ save: true })}
            disabled={write.isPending}
          >
            Save to flash
          </Button>
        </div>
        {write.isError && (
          <div className="mt-1 text-xs text-destructive">
            {(write.error as ApiError).message}
          </div>
        )}
        {confirm && (
          <ConfirmDialog
            title={confirm.save ? "Save to flash" : "Write to RAM"}
            description={
              <>
                You are about to {confirm.save ? "save " : "write "}
                <code className="font-mono">{param.name}</code> ={" "}
                <code className="font-mono">{draft}</code>{" "}
                {param.units ?? ""} on{" "}
                <code className="font-mono">{role}</code>.
                {confirm.save && " This persists across power cycles."}
              </>
            }
            confirmLabel={confirm.save ? "Save" : "Write"}
            confirmVariant="destructive"
            onCancel={() => setConfirm(null)}
            onConfirm={() => write.mutate({ save: confirm.save })}
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
