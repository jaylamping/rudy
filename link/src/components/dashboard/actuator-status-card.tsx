// One-glance roll-up of every motor in the inventory: fault/warn flags,
// recency of telemetry, and per-motor temp. Click-through goes to the
// existing Telemetry route for full charts.

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ArrowRight } from "lucide-react";
import { api } from "@/lib/api";
import {
  bootStateDotClass,
  bootStateRoleTextClass,
  bootStateShortLabel,
  bootStateSortRank,
} from "@/lib/bootStateUi";
import { useLiveInterval } from "@/lib/hooks/useLiveInterval";
import { cn } from "@/lib/utils";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import { DashboardCard } from "./dashboard-card";

const STALE_MS = 3_000;
const HOT_DEGC = 65;

type Tone = "ok" | "warn" | "crit" | "stale" | "missing";

export function ActuatorStatusCard({ className }: { className?: string }) {
  // Live data flows in via the WebTransport bridge (see
  // `lib/hooks/WebTransportBridge.tsx`), which writes freshest-per-role
  // MotorFeedback into this query's cache every animation frame. The REST
  // poll is a slow safety net for dropped datagrams and the disconnected
  // fallback for environments without WebTransport (Vite dev, non-Chromium).
  const q = useQuery({
    queryKey: ["motors"],
    queryFn: () => api.listMotors(),
    refetchInterval: useLiveInterval({ live: 30_000, fallback: 2_000 }),
  });

  const motors = q.data ?? [];
  const sortedMotors = useMemo(
    () =>
      [...motors].sort((a, b) => {
        const d =
          bootStateSortRank(a.boot_state) - bootStateSortRank(b.boot_state);
        if (d !== 0) return d;
        return a.role.localeCompare(b.role);
      }),
    [motors],
  );
  const tally = countByTone(motors);

  return (
    <DashboardCard
      title="Actuators"
      className={className}
      hint={
        <Link
          to="/telemetry"
          className="flex items-center gap-1 text-muted-foreground hover:text-foreground"
        >
          telemetry <ArrowRight className="h-3 w-3" />
        </Link>
      }
    >
      <div className="mb-3 flex items-center gap-3 text-xs">
        <Pill tone="ok">{tally.ok} ok</Pill>
        {tally.warn > 0 && <Pill tone="warn">{tally.warn} warn</Pill>}
        {tally.crit > 0 && <Pill tone="crit">{tally.crit} fault</Pill>}
        {tally.stale > 0 && <Pill tone="stale">{tally.stale} stale</Pill>}
        {tally.missing > 0 && (
          <Pill tone="missing">{tally.missing} no data</Pill>
        )}
      </div>

      {q.isPending && (
        <div className="text-sm text-muted-foreground">loading...</div>
      )}
      {q.isError && (
        <div className="text-sm text-destructive">
          {(q.error as Error).message}
        </div>
      )}
      {q.isSuccess && motors.length === 0 && (
        <div className="text-sm text-muted-foreground">
          No motors in inventory.
        </div>
      )}

      <ul>
        {sortedMotors.map((m) => (
          <MotorRow key={m.role} motor={m} />
        ))}
      </ul>
    </DashboardCard>
  );
}

function MotorRow({ motor }: { motor: MotorSummary }) {
  const fb = motor.latest;
  const ageS = fb ? (Date.now() - Number(fb.t_ms)) / 1000 : null;
  const bs = motor.boot_state;
  const bootDot = bootStateDotClass(bs);
  const roleColor = bootStateRoleTextClass(bs);
  const bootLabel = bootStateShortLabel(bs);

  return (
    <li>
      <Link
        to="/actuators/$role"
        params={{ role: motor.role }}
        className="-mx-2 flex items-center justify-between rounded-md px-2 py-1.5 text-xs transition-colors hover:bg-accent/40 focus-visible:bg-accent/40 focus-visible:outline-none"
      >
        <div className="flex min-w-0 flex-1 items-center gap-2 truncate">
          <span
            className={cn("h-2 w-2 shrink-0 rounded-full", bootDot)}
            title={`Boot: ${bootLabel}`}
          />
          <span
            className={cn("truncate font-semibold", roleColor)}
            title={`${motor.role} · ${bootLabel}`}
          >
            {motor.role}
          </span>
          <span className="shrink-0 text-[0.65rem] font-medium opacity-90">
            <span className={roleColor}>{bootLabel}</span>
          </span>
          <span className="truncate text-muted-foreground">
            0x{motor.can_id.toString(16).padStart(2, "0").toUpperCase()} ·{" "}
            {motor.can_bus}
          </span>
        </div>
        <div className="flex items-center gap-3 font-mono tabular-nums text-muted-foreground">
          {fb ? (
            <>
              <span title="position (rad)">{fb.mech_pos_rad.toFixed(2)}</span>
              <span
                title="temperature"
                className={cn(
                  fb.temp_c >= HOT_DEGC && "text-amber-400",
                  fb.temp_c >= HOT_DEGC + 10 && "text-rose-400",
                )}
              >
                {fb.temp_c.toFixed(0)}degC
              </span>
              <span
                title="last update"
                className={cn(
                  ageS != null && ageS * 1000 > STALE_MS && "text-amber-400",
                )}
              >
                {fmtAge(ageS)}
              </span>
            </>
          ) : (
            <span className="italic">no data</span>
          )}
        </div>
      </Link>
    </li>
  );
}

function getTone(m: MotorSummary): Tone {
  const fb = m.latest;
  if (!fb) return "missing";
  if (fb.fault_sta !== 0) return "crit";
  if (fb.warn_sta !== 0) return "warn";
  if (Date.now() - Number(fb.t_ms) > STALE_MS) return "stale";
  return "ok";
}

function countByTone(motors: MotorSummary[]) {
  const c = { ok: 0, warn: 0, crit: 0, stale: 0, missing: 0 };
  for (const m of motors) c[getTone(m)] += 1;
  return c;
}

function Pill({
  children,
  tone,
}: {
  children: React.ReactNode;
  tone: Tone;
}) {
  return (
    <span
      className={cn(
        "rounded-sm px-1.5 py-0.5",
        tone === "ok" && "bg-emerald-500/10 text-emerald-400",
        tone === "warn" && "bg-amber-500/10 text-amber-400",
        tone === "crit" && "bg-rose-500/10 text-rose-400",
        tone === "stale" && "bg-amber-500/10 text-amber-400",
        tone === "missing" && "bg-muted text-muted-foreground",
      )}
    >
      {children}
    </span>
  );
}

function fmtAge(s: number | null): string {
  if (s == null) return "--";
  if (s < 1) return `${Math.max(0, Math.round(s * 1000))}ms`;
  if (s < 60) return `${s.toFixed(1)}s`;
  return `${Math.floor(s / 60)}m`;
}
