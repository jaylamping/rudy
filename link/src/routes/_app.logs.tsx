import { useEffect, useMemo, useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { LIVE_LOGS_KEY } from "@/lib/hooks/wtReducers";
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

function LogsPage() {
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  const qc = useQueryClient();

  const filters: LogFilterValue = useMemo(
    () => ({
      levels: parseCsvSet<LogLevel>(search.level, ALL_LEVELS, DEFAULT_LEVELS),
      sources: parseCsvSet<LogSource>(search.source, ALL_SOURCES),
      q: search.q ?? "",
      target: search.target ?? "",
    }),
    [search.level, search.source, search.q, search.target],
  );
  const follow = search.follow ?? true;

  // Live tail comes from the WT reducer, which writes the cache key
  // `LIVE_LOGS_KEY`. We seed the cache from a REST page so the tail is
  // populated even if the WT bridge is still connecting.
  const liveQ = useQuery<LogEntry[]>({
    queryKey: LIVE_LOGS_KEY,
    queryFn: async () => {
      // Pull a starter page; the reducer will prepend live entries on
      // top from now on. Limit 500 keeps the initial paint snappy and
      // matches the cap used by older `dmesg`-style consoles.
      const res = await api.logs.list({ limit: 500 });
      return res.entries;
    },
    staleTime: Number.POSITIVE_INFINITY, // live data; never refetch on its own
  });

  const allEntries = liveQ.data ?? [];

  // Apply client-side filters on top of the live tail. Doing it on the
  // client avoids re-fetching every time the operator toggles a level
  // pill; the cap of 5000 entries keeps the cost negligible.
  const filtered = useMemo(() => {
    const q = filters.q.trim().toLowerCase();
    const t = filters.target.trim().toLowerCase();
    return allEntries.filter((e) => {
      if (!filters.levels.has(e.level)) return false;
      if (!filters.sources.has(e.source)) return false;
      if (q && !e.message.toLowerCase().includes(q)) return false;
      if (t && !e.target.toLowerCase().includes(t)) return false;
      return true;
    });
  }, [allEntries, filters]);

  const [selectedId, setSelectedId] = useState<bigint | null>(null);
  const selectedEntry = useMemo(
    () => (selectedId === null ? null : filtered.find((e) => e.id === selectedId) ?? null),
    [filtered, selectedId],
  );

  // If the operator clears their selection by filtering it out, drop it.
  useEffect(() => {
    if (selectedId !== null && !filtered.some((e) => e.id === selectedId)) {
      setSelectedId(null);
    }
  }, [filtered, selectedId]);

  const clearMut = useMutation({
    mutationFn: () => api.logs.clear(),
    onSuccess: () => {
      qc.setQueryData<LogEntry[]>(LIVE_LOGS_KEY, []);
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
