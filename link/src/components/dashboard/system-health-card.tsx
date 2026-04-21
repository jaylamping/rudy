// Pi CPU / memory / temperatures / throttle state, polled from /api/system.

import { useQuery } from "@tanstack/react-query";
import { queryKeys } from "@/api";
import { api } from "@/lib/api";
import { useLiveInterval } from "@/lib/hooks/useLiveInterval";
import { cn } from "@/lib/utils";
import { DashboardCard } from "./dashboard-card";

export function SystemHealthCard({ className }: { className?: string }) {
  // Live updates push in via the WebTransport bridge (system snapshots are
  // broadcast at ~2 s on the daemon side). REST stays as the bootstrap +
  // disconnected fallback.
  const q = useQuery({
    queryKey: queryKeys.system(),
    queryFn: () => api.system(),
    refetchInterval: useLiveInterval({ live: 30_000, fallback: 5_000 }),
  });

  const snap = q.data;
  const memPct =
    snap && snap.mem_total_mb > 0n
      ? (Number(snap.mem_used_mb) / Number(snap.mem_total_mb)) * 100
      : 0;

  return (
    <DashboardCard
      title="System health"
      className={className}
      hint={
        snap?.is_mock ? (
          <span className="rounded-sm bg-amber-500/10 px-1.5 py-0.5 text-amber-400">
            mock
          </span>
        ) : (
          snap?.hostname && <span>{snap.hostname}</span>
        )
      }
    >
      {q.isPending && (
        <div className="text-sm text-muted-foreground">loading...</div>
      )}
      {q.isError && (
        <div className="text-sm text-destructive">
          {(q.error as Error).message}
        </div>
      )}
      {snap && (
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <Gauge label="CPU" value={snap.cpu_pct} unit="%" warn={75} crit={90} />
          <Gauge label="MEM" value={memPct} unit="%" warn={75} crit={90} />
          <Gauge
            label="CPU temp"
            value={snap.temps_c.cpu ?? null}
            unit="degC"
            warn={70}
            crit={80}
          />
          <Gauge
            label="GPU temp"
            value={snap.temps_c.gpu ?? null}
            unit="degC"
            warn={70}
            crit={80}
          />
          <Stat label="Load 1m" value={snap.load[0].toFixed(2)} mono />
          <Stat
            label="Mem used"
            value={`${snap.mem_used_mb} / ${snap.mem_total_mb} MiB`}
          />
          <Stat label="Uptime" value={fmtUptime(Number(snap.uptime_s))} />
          <Stat
            label="Throttled"
            value={
              snap.throttled.now
                ? "now"
                : snap.throttled.ever
                  ? "ever"
                  : "no"
            }
            tone={
              snap.throttled.now
                ? "crit"
                : snap.throttled.ever
                  ? "warn"
                  : "ok"
            }
          />
        </div>
      )}
    </DashboardCard>
  );
}

function Gauge({
  label,
  value,
  unit,
  warn,
  crit,
}: {
  label: string;
  value: number | null;
  unit: string;
  warn: number;
  crit: number;
}) {
  const tone =
    value === null
      ? "muted"
      : value >= crit
        ? "crit"
        : value >= warn
          ? "warn"
          : "ok";
  const pct = value === null ? 0 : Math.max(0, Math.min(100, value));
  return (
    <div className="rounded-md border border-border/60 bg-background px-2 py-1.5">
      <div className="flex items-baseline justify-between">
        <span className="text-xs text-muted-foreground">{label}</span>
        <span className={cn("font-mono text-xs tabular-nums", toneClass(tone))}>
          {value === null ? "--" : value.toFixed(unit === "%" ? 0 : 1)} {unit}
        </span>
      </div>
      <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-border">
        <div
          className={cn("h-full transition-all", toneBg(tone))}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
  tone = "muted",
  mono,
}: {
  label: string;
  value: string;
  tone?: "ok" | "warn" | "crit" | "muted";
  mono?: boolean;
}) {
  return (
    <div className="rounded-md border border-border/60 bg-background px-2 py-1.5">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className={cn("text-xs", mono && "font-mono tabular-nums", toneClass(tone))}>
        {value}
      </div>
    </div>
  );
}

function toneClass(tone: "ok" | "warn" | "crit" | "muted") {
  switch (tone) {
    case "ok":
      return "text-emerald-400";
    case "warn":
      return "text-amber-400";
    case "crit":
      return "text-rose-400";
    case "muted":
    default:
      return "text-foreground";
  }
}

function toneBg(tone: "ok" | "warn" | "crit" | "muted") {
  switch (tone) {
    case "ok":
      return "bg-emerald-500";
    case "warn":
      return "bg-amber-500";
    case "crit":
      return "bg-rose-500";
    case "muted":
    default:
      return "bg-muted-foreground";
  }
}

function fmtUptime(s: number): string {
  if (!Number.isFinite(s) || s < 0) return "--";
  const d = Math.floor(s / 86_400);
  const h = Math.floor((s % 86_400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}
