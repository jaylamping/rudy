// CRUD-on-a-card for /api/reminders. Optimistic updates would be nicer
// but the server is on-box; we just invalidate on success. Mutations are
// audit-logged server-side.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { queryKeys } from "@/api";
import { api } from "@/lib/api";
import { cn } from "@/lib/utils";
import type { Reminder } from "@/lib/types/Reminder";
import { DashboardCard } from "./dashboard-card";

export function RemindersCard({ className }: { className?: string }) {
  const qc = useQueryClient();
  const [draft, setDraft] = useState("");
  const [dueAt, setDueAt] = useState("");

  const list = useQuery({
    queryKey: queryKeys.reminders.all(),
    queryFn: () => api.reminders.list(),
    refetchInterval: 15_000,
  });

  const invalidate = () =>
    qc.invalidateQueries({ queryKey: queryKeys.reminders.all() });

  const create = useMutation({
    mutationFn: () =>
      api.reminders.create({
        text: draft.trim(),
        due_at: dueAt ? new Date(dueAt).toISOString() : null,
        done: false,
      }),
    onSuccess: () => {
      setDraft("");
      setDueAt("");
      invalidate();
    },
  });

  const toggle = useMutation({
    mutationFn: (r: Reminder) =>
      api.reminders.update(r.id, {
        text: r.text,
        due_at: r.due_at,
        done: !r.done,
      }),
    onSuccess: invalidate,
  });

  const remove = useMutation({
    mutationFn: (id: string) => api.reminders.delete(id),
    onSuccess: invalidate,
  });

  const items = sortReminders(list.data ?? []);

  return (
    <DashboardCard
      title="Reminders"
      className={className}
      hint={
        items.length > 0 && (
          <span>
            {items.filter((r) => !r.done).length} open / {items.length}
          </span>
        )
      }
    >
      <form
        className="mb-3 flex flex-wrap gap-1.5"
        onSubmit={(e) => {
          e.preventDefault();
          if (!draft.trim() || create.isPending) return;
          create.mutate();
        }}
      >
        <input
          type="text"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Tighten left elbow ..."
          className="min-w-0 flex-1 rounded-md border border-border bg-background px-2 py-1 text-sm outline-none ring-0 focus:border-accent-foreground/40"
        />
        <input
          type="datetime-local"
          value={dueAt}
          onChange={(e) => setDueAt(e.target.value)}
          className="rounded-md border border-border bg-background px-2 py-1 text-xs text-muted-foreground"
          title="Optional due date"
        />
        <button
          type="submit"
          disabled={!draft.trim() || create.isPending}
          className={cn(
            "flex items-center gap-1 rounded-md border border-border bg-accent px-2 py-1 text-xs text-accent-foreground transition",
            (!draft.trim() || create.isPending) &&
              "cursor-not-allowed opacity-50",
          )}
        >
          <Plus className="h-3.5 w-3.5" /> Add
        </button>
      </form>
      {create.isError && (
        <div className="mb-2 text-xs text-destructive">
          {(create.error as Error).message}
        </div>
      )}

      {list.isPending && (
        <div className="text-sm text-muted-foreground">loading...</div>
      )}
      {list.isSuccess && items.length === 0 && (
        <div className="text-sm text-muted-foreground">
          No reminders. Add one above.
        </div>
      )}

      <ul className="space-y-1">
        {items.map((r) => (
          <li
            key={r.id}
            className={cn(
              "flex items-center gap-2 rounded-md border border-border/60 bg-background px-2 py-1.5 text-sm",
              r.done && "opacity-60",
            )}
          >
            <button
              type="button"
              onClick={() => toggle.mutate(r)}
              disabled={toggle.isPending}
              className={cn(
                "flex h-5 w-5 shrink-0 items-center justify-center rounded border border-border transition",
                r.done
                  ? "bg-emerald-500/80 text-background"
                  : "hover:bg-accent/60",
              )}
              title={r.done ? "Mark not done" : "Mark done"}
            >
              {r.done && <Check className="h-3.5 w-3.5" />}
            </button>
            <div className="min-w-0 flex-1">
              <div
                className={cn(
                  "truncate",
                  r.done && "line-through text-muted-foreground",
                )}
              >
                {r.text}
              </div>
              {r.due_at && (
                <div
                  className={cn(
                    "text-xs",
                    overdue(r) ? "text-rose-400" : "text-muted-foreground",
                  )}
                >
                  {fmtRelative(r.due_at)}
                </div>
              )}
            </div>
            <button
              type="button"
              onClick={() => remove.mutate(r.id)}
              disabled={remove.isPending}
              className="rounded p-1 text-muted-foreground transition hover:bg-rose-500/10 hover:text-rose-400"
              title="Delete"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </li>
        ))}
      </ul>
    </DashboardCard>
  );
}

function sortReminders(rs: Reminder[]): Reminder[] {
  return [...rs].sort((a, b) => {
    if (a.done !== b.done) return a.done ? 1 : -1;
    const ad = a.due_at ? Date.parse(a.due_at) : Infinity;
    const bd = b.due_at ? Date.parse(b.due_at) : Infinity;
    if (ad !== bd) return ad - bd;
    return Number(b.created_ms) - Number(a.created_ms);
  });
}

function overdue(r: Reminder): boolean {
  if (!r.due_at || r.done) return false;
  const t = Date.parse(r.due_at);
  return Number.isFinite(t) && t < Date.now();
}

function fmtRelative(iso: string): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return iso;
  const dMs = t - Date.now();
  const past = dMs < 0;
  const abs = Math.abs(dMs);
  const m = Math.round(abs / 60_000);
  const h = Math.round(abs / 3_600_000);
  const d = Math.round(abs / 86_400_000);
  let body: string;
  if (m < 60) body = `${m}m`;
  else if (h < 48) body = `${h}h`;
  else body = `${d}d`;
  return past ? `overdue by ${body}` : `in ${body}`;
}
