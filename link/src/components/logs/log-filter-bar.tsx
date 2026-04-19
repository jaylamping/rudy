import { Pause, Play, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import type { LogLevel } from "@/lib/types/LogLevel";
import type { LogSource } from "@/lib/types/LogSource";

const LEVELS: LogLevel[] = ["trace", "debug", "info", "warn", "error"];
const SOURCES: LogSource[] = ["tracing", "audit"];

export interface LogFilterValue {
  levels: Set<LogLevel>;
  sources: Set<LogSource>;
  q: string;
  target: string;
}

export const ALL_LEVELS = new Set<LogLevel>(LEVELS);
export const ALL_SOURCES = new Set<LogSource>(SOURCES);

/** Top filter bar: severity toggles, source toggles, freetext search,
 * pause/resume live tail, and clear-all. The actual REST query / live
 * tail filtering happens in the page; this component is a controlled
 * input. */
export function LogFilterBar({
  value,
  onChange,
  follow,
  onToggleFollow,
  onClear,
  totalShown,
  totalLive,
}: {
  value: LogFilterValue;
  onChange: (next: LogFilterValue) => void;
  follow: boolean;
  onToggleFollow: () => void;
  onClear: () => void;
  totalShown: number;
  totalLive: number;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-border bg-card px-3 py-2">
      <div className="flex items-center gap-1">
        {LEVELS.map((lv) => (
          <PillToggle
            key={lv}
            label={lv}
            active={value.levels.has(lv)}
            onClick={() => {
              const levels = new Set(value.levels);
              if (levels.has(lv)) levels.delete(lv);
              else levels.add(lv);
              onChange({ ...value, levels });
            }}
          />
        ))}
      </div>

      <div className="mx-2 h-5 w-px bg-border" />

      <div className="flex items-center gap-1">
        {SOURCES.map((s) => (
          <PillToggle
            key={s}
            label={s}
            active={value.sources.has(s)}
            onClick={() => {
              const sources = new Set(value.sources);
              if (sources.has(s)) sources.delete(s);
              else sources.add(s);
              onChange({ ...value, sources });
            }}
          />
        ))}
      </div>

      <div className="mx-2 h-5 w-px bg-border" />

      <Input
        placeholder="search messages…"
        value={value.q}
        onChange={(e) => onChange({ ...value, q: e.target.value })}
        className="h-7 w-48"
      />
      <Input
        placeholder="target filter…"
        value={value.target}
        onChange={(e) => onChange({ ...value, target: e.target.value })}
        className="h-7 w-40"
      />

      <div className="ml-auto flex items-center gap-2">
        <span className="font-mono text-xs text-muted-foreground">
          {totalShown.toLocaleString()} / {totalLive.toLocaleString()}
        </span>
        <Button
          variant={follow ? "default" : "outline"}
          size="sm"
          onClick={onToggleFollow}
          title={follow ? "Pause live tail" : "Resume live tail"}
          className="h-7 gap-1"
        >
          {follow ? <Pause className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
          <span className="text-xs">{follow ? "Live" : "Paused"}</span>
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={onClear}
          title="Clear the persistent log store"
          className="h-7 gap-1 text-rose-300 hover:text-rose-200"
        >
          <Trash2 className="h-3.5 w-3.5" />
          <span className="text-xs">Clear</span>
        </Button>
      </div>
    </div>
  );
}

function PillToggle({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "rounded-sm px-2 py-0.5 font-mono text-[10px] uppercase tracking-wide transition-colors",
        active
          ? "bg-primary text-primary-foreground"
          : "bg-muted text-muted-foreground hover:bg-muted/70",
      )}
    >
      {label}
    </button>
  );
}
