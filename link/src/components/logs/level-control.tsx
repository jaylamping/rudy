import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, RotateCcw, Save } from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

/** Editor for the runtime `EnvFilter` directive string. Reads
 * `/api/logs/level`, lets the operator edit the raw directive, and
 * PUTs it on Save. The daemon validates with `EnvFilter::try_new`
 * before applying — invalid strings come back as a 400 with a `detail`
 * we surface inline. */
export function LevelControl() {
  const qc = useQueryClient();
  const stateQ = useQuery({
    queryKey: ["logs", "level"],
    queryFn: () => api.logs.getLevel(),
  });

  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  // Sync the draft with server state when it (re)loads or after a save.
  // We don't want to clobber an in-progress edit, so only overwrite when
  // the input is empty — which it is on first load.
  useEffect(() => {
    if (stateQ.data && draft === "") setDraft(stateQ.data.raw);
  }, [stateQ.data, draft]);

  const setMut = useMutation({
    mutationFn: (raw: string) => api.logs.setLevel(raw),
    onSuccess: (next) => {
      qc.setQueryData(["logs", "level"], next);
      setDraft(next.raw);
      setError(null);
    },
    onError: (err) => {
      if (err instanceof ApiError && err.body && typeof err.body === "object") {
        const body = err.body as { error?: string; detail?: string };
        setError(body.detail ?? body.error ?? err.message);
      } else {
        setError((err as Error).message);
      }
    },
  });

  const dirty = stateQ.data ? draft !== stateQ.data.raw : false;
  const isInfo = stateQ.data?.default ?? "info";

  return (
    <div className="space-y-2 border-b border-border bg-muted/30 px-3 py-2">
      <div className="flex items-center gap-2">
        <span className="text-xs uppercase tracking-wide text-muted-foreground">
          tracing filter
        </span>
        <span
          className={cn(
            "rounded-sm bg-muted px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide",
            isInfo === "trace" || isInfo === "debug" ? "text-amber-300" : "text-muted-foreground",
          )}
        >
          default = {isInfo}
        </span>

        <Input
          spellCheck={false}
          autoComplete="off"
          className="h-7 flex-1 font-mono text-xs"
          value={draft}
          placeholder="info,rudydae=debug,wtransport=warn"
          onChange={(e) => {
            setDraft(e.target.value);
            if (error) setError(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && dirty && !setMut.isPending) {
              setMut.mutate(draft.trim());
            }
          }}
          disabled={stateQ.isPending || setMut.isPending}
        />

        <Button
          variant="outline"
          size="sm"
          className="h-7 gap-1"
          disabled={!dirty || setMut.isPending}
          onClick={() => setMut.mutate(draft.trim())}
          title="Apply this directive at runtime and persist for next boot"
        >
          {setMut.isPending ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Save className="h-3.5 w-3.5" />
          )}
          <span className="text-xs">Apply</span>
        </Button>

        <Button
          variant="ghost"
          size="sm"
          className="h-7 gap-1"
          disabled={!stateQ.data || !dirty || setMut.isPending}
          onClick={() => {
            if (stateQ.data) setDraft(stateQ.data.raw);
            setError(null);
          }}
          title="Revert unsaved edits"
        >
          <RotateCcw className="h-3.5 w-3.5" />
          <span className="text-xs">Revert</span>
        </Button>
      </div>

      {error && (
        <div className="rounded-sm border border-rose-500/40 bg-rose-500/10 px-2 py-1 font-mono text-[11px] text-rose-200">
          {error}
        </div>
      )}
    </div>
  );
}
