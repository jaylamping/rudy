import type { LogLevel } from "@/lib/types/LogLevel";
import { cn } from "@/lib/utils";

/** One small colored badge per log level. Single source of truth for the
 * level→color mapping; both the row and the detail panel read from here
 * so a future palette tweak is one file. */
const TONE: Record<LogLevel, string> = {
  trace: "bg-muted text-muted-foreground",
  debug: "bg-muted text-foreground",
  info: "bg-sky-500/15 text-sky-300 dark:text-sky-200",
  warn: "bg-amber-500/15 text-amber-300 dark:text-amber-200",
  error: "bg-rose-500/15 text-rose-300 dark:text-rose-200",
};

export function LevelBadge({ level, className }: { level: LogLevel; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex items-center justify-center rounded-sm px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide",
        TONE[level],
        className,
      )}
    >
      {level}
    </span>
  );
}
