import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Copy, Download, RefreshCw, RotateCcw, ShieldCheck, SlidersHorizontal } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { queryKeys, settingsQueryOptions } from "@/api";
import { api, ApiError } from "@/lib/api";
import type { SettingEntry } from "@/lib/types/SettingEntry";
import type { SettingsGetResponse } from "@/lib/types/SettingsGetResponse";
import type { SettingsApplyMode } from "@/lib/types/SettingsApplyMode";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/params/confirm-dialog";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

export const Route = createFileRoute("/_app/settings")({
  component: SettingsPage,
});

function SettingsPage() {
  const qc = useQueryClient();
  const q = useQuery(settingsQueryOptions());
  const [qFilter, setQFilter] = useState("");
  const [cat, setCat] = useState<"all" | string>("all");
  const [resOpen, setResOpen] = useState(false);
  const [rseedOpen, setRseedOpen] = useState(false);
  const [copied, setCopied] = useState(false);

  const resetAll = useMutation({
    mutationFn: () => api.settings.reset(),
    onSuccess: () => void qc.invalidateQueries({ queryKey: queryKeys.settings.all() }),
  });
  const reseed = useMutation({
    mutationFn: () => api.settings.reseed(),
    onSuccess: () => void qc.invalidateQueries({ queryKey: queryKeys.settings.all() }),
  });

  const categories = useMemo(() => {
    const s = new Set<string>();
    for (const e of q.data?.entries ?? []) s.add(e.category);
    return Array.from(s).sort();
  }, [q.data?.entries]);

  const filtered = useMemo(() => {
    let e = q.data?.entries ?? [];
    if (cat !== "all") e = e.filter((x) => x.category === cat);
    const t = qFilter.trim().toLowerCase();
    if (t) {
      e = e.filter(
        (x) =>
          x.key.toLowerCase().includes(t) ||
          x.label.toLowerCase().includes(t) ||
          x.description.toLowerCase().includes(t),
      );
    }
    return e;
  }, [q.data?.entries, cat, qFilter]);

  return (
    <div className="flex h-full min-h-0 flex-col gap-3 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <SlidersHorizontal className="h-5 w-5 text-muted-foreground" />
        <h1 className="text-lg font-semibold">Settings</h1>
        <Badge variant="secondary" className="text-xs">
          cortex
        </Badge>
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!q.data && q.isFetching}
            onClick={async () => {
              const fresh = await q.refetch();
              const data = fresh.data ?? q.data;
              if (!data) return;
              await copySettingsExport(data);
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1500);
            }}
          >
            <Copy className="mr-1 h-3.5 w-3.5" />
            {copied ? "Copied" : "Copy JSON"}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!q.data && q.isFetching}
            onClick={async () => {
              const fresh = await q.refetch();
              const data = fresh.data ?? q.data;
              if (data) downloadSettingsExport(data);
            }}
          >
            <Download className="mr-1 h-3.5 w-3.5" />
            Export JSON
          </Button>
          {q.data?.runtime_db_enabled ? (
            <>
              <Button type="button" variant="outline" size="sm" onClick={() => setResOpen(true)}>
                <RotateCcw className="mr-1 h-3.5 w-3.5" />
                Reset to TOML seed
              </Button>
              <Button type="button" variant="outline" size="sm" onClick={() => setRseedOpen(true)}>
                Reseed
              </Button>
            </>
          ) : null}
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void q.refetch()}
            disabled={q.isFetching}
          >
            <RefreshCw className="mr-1 h-3.5 w-3.5" />
            Refresh
          </Button>
        </div>
      </div>

      {q.data?.recovery_pending ? (
        <div className="flex flex-col gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm">
          <div className="font-medium text-destructive">Recovery pending</div>
          <div className="flex flex-wrap items-center gap-2 text-muted-foreground">
            Runtime DB re-seeded from files; motion blocked until acknowledge.
            <Button
              type="button"
              size="sm"
              onClick={async () => {
                await api.settings.recoveryAck();
                await qc.invalidateQueries({ queryKey: queryKeys.settings.all() });
              }}
            >
              <ShieldCheck className="mr-1 h-3.5 w-3.5" />
              Acknowledge
            </Button>
          </div>
        </div>
      ) : null}

      {q.isError ? (
        <p className="text-sm text-destructive">
          {(q.error as ApiError).message}
        </p>
      ) : null}

      <div className="grid gap-2 md:grid-cols-[1fr,180px,180px]">
        <Input
          placeholder="Search key / label / description"
          value={qFilter}
          onChange={(e) => setQFilter(e.target.value)}
          className="h-8 text-sm"
        />
        <select
          className="h-8 rounded-md border border-input bg-background px-2 text-sm"
          value={cat}
          onChange={(e) => setCat(e.target.value as "all" | string)}
        >
          <option value="all">All categories</option>
          {categories.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
        <div className="self-center text-xs text-muted-foreground">
          {q.data
            ? `Runtime DB: ${q.data.runtime_db_enabled ? "on" : "off"} — ${q.data.entries.length} keys`
            : null}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto rounded border border-border">
        <table className="w-full min-w-[900px] border-collapse text-left text-sm">
          <thead className="sticky top-0 z-10 border-b border-border bg-muted/80 text-xs text-muted-foreground">
            <tr>
              <th className="p-2 font-medium">Key</th>
              <th className="p-2 font-medium">Value</th>
              <th className="p-2 font-medium">Seed</th>
              <th className="p-2 font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {q.isSuccess ? filtered.map((e) => <EntryRow key={e.key} entry={e} />) : null}
          </tbody>
        </table>
      </div>

      {resOpen ? (
        <ConfirmDialog
          title="Reset all settings to TOML seed?"
          description="Replaces settings_kv with values from cortex.toml. Requires no motors enabled."
          confirmLabel="Reset"
          confirmVariant="destructive"
          onCancel={() => setResOpen(false)}
          onConfirm={() => {
            void resetAll.mutateAsync().catch(() => {});
            setResOpen(false);
          }}
        />
      ) : null}
      {rseedOpen ? (
        <ConfirmDialog
          title="Re-seed from TOML?"
          description="Same as reset; use after confirming X-Rudy-Reseed-Confirm is understood."
          confirmLabel="Reseed"
          confirmVariant="destructive"
          onCancel={() => setRseedOpen(false)}
          onConfirm={() => {
            void reseed.mutateAsync().catch(() => {});
            setRseedOpen(false);
          }}
        />
      ) : null}
    </div>
  );
}

type SettingsExportData = {
  runtime_db_enabled: boolean;
  recovery_pending: boolean;
  entries: SettingEntry[];
};

type SettingJsonValue = SettingsGetResponse["entries"][number]["effective"];

function settingsExportJson(data: SettingsExportData): string {
  const exportedAt = new Date().toISOString();
  const values = Object.fromEntries(data.entries.map((e) => [e.key, e.effective]));
  const payload = {
    exported_at: exportedAt,
    runtime_db_enabled: data.runtime_db_enabled,
    recovery_pending: data.recovery_pending,
    values,
    entries: data.entries.map((e) => ({
      key: e.key,
      label: e.label,
      category: e.category,
      value_kind: e.value_kind,
      unit: e.unit,
      effective: e.effective,
      seed: e.seed,
      in_db: e.in_db,
      dirty: e.dirty,
      apply_mode: e.apply_mode,
      editable: e.editable,
      read_only_reason: e.read_only_reason,
      requires_motors_stopped: e.requires_motors_stopped,
    })),
  };
  return `${JSON.stringify(payload, null, 2)}\n`;
}

function downloadSettingsExport(data: SettingsExportData) {
  const exportedAt = new Date().toISOString();
  const blob = new Blob([settingsExportJson(data)], {
    type: "application/json",
  });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `rudy-settings-${exportedAt.replace(/[:.]/g, "-")}.json`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

async function copySettingsExport(data: SettingsExportData) {
  const text = settingsExportJson(data);
  if (navigator.clipboard) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand("copy");
  textarea.remove();
}

function EntryRow({ entry: e }: { entry: SettingEntry }) {
  const qc = useQueryClient();
  const [draft, setDraft] = useState(() => valueForEdit(e));
  const [saveNotice, setSaveNotice] = useState<string | null>(null);
  const [restartPrompt, setRestartPrompt] = useState<RestartPrompt | null>(null);

  useEffect(() => {
    setDraft(valueForEdit(e));
  }, [e]);

  const put = useMutation({
    mutationFn: async () => {
      const v = parseValue(e, draft);
      return api.settings.put(e.key, { value: v });
    },
    onMutate: () => {
      setSaveNotice(null);
      setRestartPrompt(null);
    },
    onSuccess: async (saved) => {
      qc.setQueryData<SettingsGetResponse>(queryKeys.settings.all(), (old) =>
        old ? settingsWithSavedEntry(old, saved.key, saved.effective) : old,
      );
      const notice = applyModeNotice(saved.apply_mode, saved.note);
      setSaveNotice(notice);
      setRestartPrompt(restartPromptForSave(saved.apply_mode, notice, saved.key));
      await qc.invalidateQueries({ queryKey: queryKeys.settings.all() });
    },
  });

  if (!e.editable) {
    return (
      <tr className="align-top border-b border-border/60">
        <td className="p-2 font-mono text-xs text-muted-foreground">
          {e.key}
          {e.read_only_reason ? (
            <div className="pt-0.5 text-[11px] text-amber-600">{e.read_only_reason}</div>
          ) : null}
        </td>
        <td className="p-2" colSpan={2}>
          <div className="flex items-start gap-1.5">
            <code className="break-all text-xs">{JSON.stringify(e.effective)}</code>
            <OverrideIndicator entry={e} />
          </div>
        </td>
        <td className="p-2 text-xs text-muted-foreground" />
      </tr>
    );
  }

  return (
    <tr className="align-top border-b border-border/60">
      <td className="p-2">
        <div className="font-mono text-xs leading-tight">{e.key}</div>
        <div className="pt-0.5 text-[11px] text-muted-foreground">{e.label}</div>
        {e.description ? (
          <div className="pt-1 text-[11px] text-muted-foreground">{e.description}</div>
        ) : null}
        <div className="flex flex-wrap gap-1 pt-1">
          {e.requires_motors_stopped ? (
            <Badge variant="secondary" className="text-[10px]">
              motors stopped
            </Badge>
          ) : null}
        </div>
      </td>
      <td className="p-2">
        <div className="flex items-start gap-1.5">
          <Editor entry={e} value={draft} onChange={setDraft} />
          <OverrideIndicator entry={e} />
        </div>
      </td>
      <td className="p-2">
        <code className="break-all text-xs text-muted-foreground">
          {JSON.stringify(e.seed)}
        </code>
      </td>
      <td className="p-2">
        <Button
          type="button"
          size="sm"
          disabled={put.isPending}
          onClick={() => void put.mutateAsync().catch(() => {})}
        >
          Save
        </Button>
        {put.isError ? (
          <div className="pt-1 text-[11px] text-destructive">
            {apiErrorMessage(put.error)}
          </div>
        ) : null}
        {saveNotice ? (
          <div className="pt-1 text-[11px] text-amber-600">{saveNotice}</div>
        ) : null}
        <div className="pt-1">
          <ResetRowToSeed entry={e} onRestartNeeded={setRestartPrompt} />
        </div>
        <RestartRequiredDialog
          prompt={restartPrompt}
          onClose={() => setRestartPrompt(null)}
        />
      </td>
    </tr>
  );
}

function OverrideIndicator({ entry: e }: { entry: SettingEntry }) {
  if (!e.in_db || !e.dirty) return null;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          aria-label="Runtime override differs from TOML seed"
          className="mt-1 inline-block h-2 w-2 shrink-0 rounded-full bg-amber-500"
        />
      </TooltipTrigger>
      <TooltipContent>Runtime override differs from TOML seed</TooltipContent>
    </Tooltip>
  );
}

function applyModeNotice(mode: SettingsApplyMode, note: string | null): string | null {
  if (mode === "runtime_immediate") return null;
  if (note) return note;
  if (mode === "requires_restart") return "Saved. Restart cortex for full effect.";
  return "Saved, but this setting is not runtime-immediate.";
}

type RestartPrompt = {
  key: string;
  message: string;
};

function restartPromptForSave(
  mode: SettingsApplyMode,
  message: string | null,
  key: string,
): RestartPrompt | null {
  if (!message || mode === "runtime_immediate") return null;
  return { key, message };
}

function RestartRequiredDialog({
  prompt,
  onClose,
}: {
  prompt: RestartPrompt | null;
  onClose: () => void;
}) {
  const restart = useMutation({
    mutationFn: () => api.restart(),
    onSuccess: () => {
      window.setTimeout(() => window.location.reload(), 2500);
    },
  });

  return (
    <Dialog open={prompt !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Restart required</DialogTitle>
          <DialogDescription>
            {prompt?.message ?? "This change needs a cortex restart for full effect."}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-2 text-sm text-muted-foreground">
          <p>
            Saved <code>{prompt?.key}</code>. Restarting drops torque on every
            present motor, exits cortex, and systemd should bring it back.
          </p>
          {restart.isSuccess ? (
            <p className="text-amber-600">
              Restart requested. Reloading shortly after cortex comes back.
            </p>
          ) : null}
          {restart.isError ? (
            <p className="text-destructive">{apiErrorMessage(restart.error)}</p>
          ) : null}
        </div>
        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={onClose}
            disabled={restart.isPending || restart.isSuccess}
          >
            Later
          </Button>
          <Button
            type="button"
            variant="destructive"
            onClick={() => restart.mutate()}
            disabled={restart.isPending || restart.isSuccess}
          >
            {restart.isPending ? "Restarting..." : "Restart now"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function settingsWithSavedEntry(
  data: SettingsGetResponse,
  key: string,
  effective: SettingJsonValue,
): SettingsGetResponse {
  return {
    ...data,
    entries: data.entries.map((entry) =>
      entry.key === key
        ? {
            ...entry,
            effective,
            in_db: true,
            dirty: JSON.stringify(effective) !== JSON.stringify(entry.seed),
          }
        : entry,
    ),
  };
}

function valueForEdit(e: SettingEntry): string {
  if (e.value_kind === "bool") return e.effective === true ? "true" : "false";
  if (e.value_kind === "option_f32" && (e.effective === null || e.effective === undefined))
    return "null";
  if (typeof e.effective === "number" || typeof e.effective === "boolean")
    return String(e.effective);
  return JSON.stringify(e.effective);
}

function parseValue(e: SettingEntry, s: string): unknown {
  const t = s.trim();
  if (e.value_kind === "bool") return t === "true";
  if (e.value_kind === "option_f32" && t === "null") return null;
  if (e.value_kind === "u32" || e.value_kind === "u64") {
    const n = Number(t);
    if (!Number.isFinite(n)) throw new Error("not a number");
    return n;
  }
  if (e.value_kind === "f32" || e.value_kind === "option_f32") {
    const n = Number(t);
    if (t === "null") return null;
    if (!Number.isFinite(n)) throw new Error("not a number");
    return n;
  }
  return JSON.parse(s) as unknown;
}

function apiErrorMessage(error: unknown): string {
  if (error instanceof ApiError) {
    const detail =
      error.body &&
      typeof error.body === "object" &&
      "detail" in error.body &&
      typeof error.body.detail === "string"
        ? error.body.detail
        : null;
    return detail ? `${error.message}: ${detail}` : error.message;
  }
  return error instanceof Error ? error.message : "Save failed";
}

function Editor({
  entry: e,
  value,
  onChange,
}: {
  entry: SettingEntry;
  value: string;
  onChange: (s: string) => void;
}) {
  if (e.value_kind === "bool") {
    const on = value === "true";
    return (
      <div className="flex items-center gap-2">
        <Switch
          checked={on}
          onCheckedChange={(c) => onChange(c ? "true" : "false")}
        />
        <span className="text-xs text-muted-foreground">{on ? "true" : "false"}</span>
      </div>
    );
  }
  if (e.value_kind === "option_f32") {
    return (
      <div className="space-y-1">
        <Input
          className="h-8 font-mono text-xs"
          value={value}
          onChange={(ev) => onChange(ev.target.value)}
        />
        <p className="text-[10px] text-muted-foreground">null clears</p>
      </div>
    );
  }
  return (
    <Input
      className="h-8 font-mono text-xs"
      value={value}
      onChange={(ev) => onChange(ev.target.value)}
    />
  );
}

function ResetRowToSeed({
  entry: e,
  onRestartNeeded,
}: {
  entry: SettingEntry;
  onRestartNeeded: (prompt: RestartPrompt | null) => void;
}) {
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const m = useMutation({
    mutationFn: () => api.settings.put(e.key, { value: e.seed as unknown }),
    onMutate: () => {
      setNotice(null);
      onRestartNeeded(null);
    },
    onSuccess: (saved) => {
      qc.setQueryData<SettingsGetResponse>(queryKeys.settings.all(), (old) =>
        old ? settingsWithSavedEntry(old, saved.key, saved.effective) : old,
      );
      const nextNotice = applyModeNotice(saved.apply_mode, saved.note);
      setNotice(nextNotice);
      onRestartNeeded(restartPromptForSave(saved.apply_mode, nextNotice, saved.key));
      void qc.invalidateQueries({ queryKey: queryKeys.settings.all() });
    },
  });
  return (
    <>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="h-7 w-7 px-0"
        aria-label={`Reset ${e.key} to seed value`}
        title="Reset to TOML seed"
        onClick={() => setOpen(true)}
      >
        <RotateCcw className="h-3 w-3" />
      </Button>
      {notice ? <div className="pt-1 text-[11px] text-amber-600">{notice}</div> : null}
      {open ? (
        <ConfirmDialog
          title="Reset to seed value?"
          description={
            <code className="font-mono text-xs break-all">{e.key}</code>
          }
          confirmLabel="Reset"
          confirmVariant="default"
          onCancel={() => setOpen(false)}
          onConfirm={() => {
            void m.mutateAsync().catch(() => {});
            setOpen(false);
          }}
        />
      ) : null}
    </>
  );
}
