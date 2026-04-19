import { useEffect, useRef } from "react";
import type { LogEntry } from "@/lib/types/LogEntry";
import { LogRow } from "./log-row";

/** Plain scrollable list (no virtualization yet). With the live cap at
 * 5000 and rows around 24px tall the scroll container holds at most a
 * few hundred onscreen rows; React handles that fine. If we ever want
 * to surface multi-day history in one view we can swap in
 * react-virtuoso without changing the parent contract. */
export function LogList({
  entries,
  selectedId,
  onSelect,
  follow,
}: {
  entries: LogEntry[];
  selectedId: bigint | null;
  onSelect: (entry: LogEntry) => void;
  /** When true, auto-scrolls to the newest row. Disabled the moment
   * the operator scrolls up, re-enabled when they scroll back to top
   * (the parent decides; we just respect the prop). */
  follow: boolean;
}) {
  const scrollerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!follow) return;
    const el = scrollerRef.current;
    if (!el) return;
    // The newest entry is at index 0 (we render newest-first), so
    // "follow tail" === "scroll to top".
    el.scrollTop = 0;
  }, [follow, entries]);

  return (
    <div
      ref={scrollerRef}
      className="h-full overflow-y-auto bg-card"
      data-testid="log-list"
    >
      {entries.length === 0 ? (
        <div className="p-6 text-sm text-muted-foreground">
          No log entries match the current filters.
        </div>
      ) : (
        entries.map((e) => (
          <LogRow
            key={String(e.id)}
            entry={e}
            selected={selectedId !== null && e.id === selectedId}
            onSelect={() => onSelect(e)}
          />
        ))
      )}
    </div>
  );
}
