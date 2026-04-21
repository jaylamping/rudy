import { useEffect, useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { queryKeys } from "@/api";
import { api } from "@/lib/api";
import { useLiveInterval } from "@/lib/hooks/useLiveInterval";
import { getBridgeWt } from "@/lib/hooks/wtBridgeHandle";
import { LIVE_LOG_CAP } from "@/lib/hooks/wtReducers";
import type { LogEntry } from "@/lib/types/LogEntry";
import type { LogLevel } from "@/lib/types/LogLevel";
import type { LogSource } from "@/lib/types/LogSource";
import {
  ALL_LEVELS,
  ALL_SOURCES,
  LevelControl,
  LogDetail,
  LogFilterBar,
  type LogFilterValue,
  LogList,
} from "@/components/logs";

interface LogsSearch {
  level?: string;   // CSV
  source?: string;  // CSV
  q?: string;
  target?: string;
  follow?: boolean;
}

export const Route = createFileRoute("/_app/logs")({
  validateSearch: (s: Record<string, unknown>): LogsSearch => ({
    level: typeof s.level === "string" ? s.level : undefined,
    source: typeof s.source === "string" ? s.source : undefined,
    q: typeof s.q === "string" ? s.q : undefined,
    target: typeof s.target === "string" ? s.target : undefined,
    follow: typeof s.follow === "boolean" ? s.follow : undefined,
  }),
  component: LogsPage,
});

function parseCsvSet<T extends string>(
  raw: string | undefined,
  all: Set<T>,
  defaultSet?: Set<T>,
): Set<T> {
  if (raw === undefined) return new Set(defaultSet ?? all);
  if (raw === "") return new Set();
  const set = new Set<T>();
  for (const part of raw.split(",")) {
    const trimmed = part.trim();
    if (all.has(trimmed as T)) set.add(trimmed as T);
  }
  return set;
}

function setEquals<T>(a: Set<T>, b: Set<T>): boolean {
  if (a.size !== b.size) return false;
  for (const v of a) if (!b.has(v)) return false;
  return true;
}

function setToCsv<T extends string>(
  set: Set<T>,
  all: Set<T>,
  defaultSet?: Set<T>,
): string | undefined {
  if (setEquals(set, defaultSet ?? all)) return undefined;
  if (set.size === 0) return "";
  return Array.from(set).join(",");
}

// `trace` and `debug` are extremely chatty (the motion loop alone fires
// several debug events per second), so we hide them by default. The
// operator can toggle them back on from the filter pill bar; the URL
// then carries an explicit CSV so the choice survives reloads.
const DEFAULT_LEVELS = new Set<LogLevel>(["info", "warn", "error"]);

/**
 * Merge a freshly-fetched REST page into the existing live tail.
 *
 * Both inputs are newest-first. We dedupe by `id`, prepend any entries
 * the REST page knows about that the cache hasn't seen yet, and cap the
 * result at `LIVE_LOG_CAP` so the in-memory tail can't grow without
 * bound. Entries the cache has but REST doesn't (older live history
 * the operator scrolled through) are preserved at the tail.
 */
function mergeLiveEntries(
  fetched: LogEntry[],
  prev: LogEntry[],
): LogEntry[] {
  if (prev.length === 0) {
    return fetched.length > LIVE_LOG_CAP ? fetched.slice(0, LIVE_LOG_CAP) : fetched;
  }
  const seen = new Set<string>();
  for (const e of prev) seen.add(String(e.id));
  const newer: LogEntry[] = [];
  for (const e of fetched) {
    if (seen.has(String(e.id))) continue;
    newer.push(e);
  }
  if (newer.length === 0) return prev;
  const merged = newer.concat(prev);
  return merged.length > LIVE_LOG_CAP ? merged.slice(0, LIVE_LOG_CAP) : merged;
}

function LogsPage() {
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  const qc = useQueryClient();

  const filters: LogFilterValue = {
    levels: parseCsvSet<LogLevel>(search.level, ALL_LEVELS, DEFAULT_LEVELS),
    sources: parseCsvSet<LogSource>(search.source, ALL_SOURCES),
    q: search.q ?? "",
    target: search.target ?? "",
  };
  const follow = search.follow ?? true;

  // Defensively widen the WT subscription on mount. Other tabs
  // (`MotionTestsCard`, `useTestProgress`) narrow the bridge filter to
  // a kind set that excludes `log_event`; their cleanup restores
  // "kinds: []", but a hot reload, StrictMode double-mount, or a WT
  // reconnect that re-applies `lastFilterRef` can leave the daemon
  // suppressing log frames for the rest of the session. The Logs page
  // is the one place we definitely want them, so re-open the firehose.
  useEffect(() => {
    const wt = getBridgeWt();
    if (!wt) return;
    void wt.setFilter({
      kinds: [],
      filters: { motor_roles: [], run_ids: [] },
    });
  }, []);

  // Live tail comes from the WT reducer, which writes the cache key
  // `queryKeys.logs.live()`. We also keep a REST safety-net poll on the
  // same key (slow when WT is healthy, fast when it isn't) so the page
  // never goes silent if the WT firehose stalls — matches the cadence
  // every other live surface uses via `useLiveInterval`.
  //
  // The queryFn merges the REST snapshot into the existing live tail
  // (instead of replacing it) so a periodic refetch can't clobber the
  // up-to-`LIVE_LOG_CAP` entries the WT reducer has accumulated.
  const refetchInterval = useLiveInterval({ live: 30_000, fallback: 2_000 });
  const liveQ = useQuery<LogEntry[]>({
    queryKey: queryKeys.logs.live(),
    queryFn: async () => {
      // Limit 500 keeps the initial paint snappy and matches the cap
      // used by older `dmesg`-style consoles.
      const res = await api.logs.list({ limit: 500 });
      const prev = qc.getQueryData<LogEntry[]>(queryKeys.logs.live()) ?? [];
      return mergeLiveEntries(res.entries, prev);
    },
    refetchInterval,
    refetchOnWindowFocus: true,
  });

  const allEntries = liveQ.data ?? [];

  // Apply client-side filters on top of the live tail. Doing it on the
  // client avoids re-fetching every time the operator toggles a level
  // pill; the cap of 5000 entries keeps the cost negligible.
  const qNorm = filters.q.trim().toLowerCase();
  const tNorm = filters.target.trim().toLowerCase();
  const filtered = allEntries.filter((e) => {
    if (!filters.levels.has(e.level)) return false;
    if (!filters.sources.has(e.source)) return false;
    if (qNorm && !e.message.toLowerCase().includes(qNorm)) return false;
    if (tNorm && !e.target.toLowerCase().includes(tNorm)) return false;
    return true;
  });

  const [selectedId, setSelectedId] = useState<bigint | null>(null);
  const selectedEntry =
    selectedId === null
      ? null
      : (filtered.find((e) => e.id === selectedId) ?? null);

  // If the operator clears their selection by filtering it out, drop it.
  useEffect(() => {
    if (selectedId !== null && !filtered.some((e) => e.id === selectedId)) {
      setSelectedId(null);
    }
  }, [filtered, selectedId]);

  const clearMut = useMutation({
    mutationFn: () => api.logs.clear(),
    onSuccess: () => {
      qc.setQueryData<LogEntry[]>(queryKeys.logs.live(), []);
      setSelectedId(null);
    },
  });

  const updateUrl = (next: Partial<LogsSearch>) => {
    void navigate({
      search: (prev) => ({ ...prev, ...next }),
      replace: true,
    });
  };

  const onFiltersChange = (next: LogFilterValue) => {
    updateUrl({
      level: setToCsv(next.levels, ALL_LEVELS, DEFAULT_LEVELS),
      source: setToCsv(next.sources, ALL_SOURCES),
      q: next.q || undefined,
      target: next.target || undefined,
    });
  };

  return (
    <div className="flex h-[calc(100vh-7rem)] flex-col gap-0 overflow-hidden rounded-lg border border-border bg-background">
      <header className="flex items-baseline justify-between border-b border-border bg-card px-3 py-2">
        <h1 className="text-lg font-semibold">Logs</h1>
        <span className="text-xs text-muted-foreground">
          captured · {allEntries.length.toLocaleString()} in client buffer
        </span>
      </header>

      <LevelControl />

      <LogFilterBar
        value={filters}
        onChange={onFiltersChange}
        follow={follow}
        onToggleFollow={() => updateUrl({ follow: !follow ? undefined : false })}
        onClear={() => {
          if (clearMut.isPending) return;
          if (window.confirm("Clear the on-disk log store? This cannot be undone.")) {
            clearMut.mutate();
          }
        }}
        totalShown={filtered.length}
        totalLive={allEntries.length}
      />

      <div className="grid min-h-0 flex-1 grid-cols-[1fr_22rem] gap-0">
        <LogList
          entries={filtered}
          selectedId={selectedId}
          onSelect={(e) => setSelectedId(e.id)}
          follow={follow && selectedId === null}
        />
        <div className="border-l border-border bg-card">
          <LogDetail entry={selectedEntry} />
        </div>
      </div>
    </div>
  );
}
