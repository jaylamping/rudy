import { cn } from "@/lib/utils";
import type { LogEntry } from "@/lib/types/LogEntry";
import { LevelBadge } from "./level-badge";

/** Format an epoch-ms timestamp to `HH:MM:SS.mmm`. Logs always render in
 * the local timezone — the operator is sitting next to the robot — and
 * the date is implicit because the page only ever shows the most-recent
 * tail. The detail panel shows the full ISO timestamp for export. */
function formatTime(t_ms: bigint | number): string {
  const d = new Date(Number(t_ms));
  const hh = d.getHours().toString().padStart(2, "0");
  const mm = d.getMinutes().toString().padStart(2, "0");
  const ss = d.getSeconds().toString().padStart(2, "0");
  const ms = d.getMilliseconds().toString().padStart(3, "0");
  return `${hh}:${mm}:${ss}.${ms}`;
}

export function LogRow({
  entry,
  selected,
  onSelect,
}: {
  entry: LogEntry;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={cn(
        "flex w-full items-start gap-2 border-b border-border/40 px-2 py-1 text-left font-mono text-[12px] leading-tight",
        "transition-colors hover:bg-muted/40",
        selected && "bg-muted/70",
      )}
    >
      <span className="shrink-0 text-muted-foreground">{formatTime(entry.t_ms)}</span>
      <LevelBadge level={entry.level} className="shrink-0 self-center" />
      <span
        className="shrink-0 truncate text-muted-foreground"
        style={{ width: "12rem" }}
        title={entry.target}
      >
        {entry.target}
      </span>
      <span className="min-w-0 flex-1 truncate">{entry.message}</span>
      {entry.source === "audit" && (
        <span className="shrink-0 self-center rounded-sm bg-violet-500/15 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-violet-200">
          audit
        </span>
      )}
    </button>
  );
}
