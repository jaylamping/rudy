import { useEffect, useRef, useState } from "react";
import uPlot, { type Options } from "uplot";
import "uplot/dist/uPlot.min.css";
import type { MotorSummary } from "@/lib/types/MotorSummary";

const WINDOW_SEC = 60;

interface Series {
  t: number[]; // seconds since first sample
  pos: number[];
  vel: number[];
  torque: number[];
  vbus: number[];
}

function newSeries(): Series {
  return { t: [], pos: [], vel: [], torque: [], vbus: [] };
}

export function TelemetryGrid({ motors }: { motors: MotorSummary[] }) {
  return (
    <div className="grid gap-4 md:grid-cols-2">
      {motors.map((m) => (
        <MotorCard key={m.role} motor={m} />
      ))}
      {motors.length === 0 && (
        <div className="col-span-2 rounded-md border border-border bg-card p-6 text-sm text-muted-foreground">
          No motors in inventory.
        </div>
      )}
    </div>
  );
}

function MotorCard({ motor }: { motor: MotorSummary }) {
  const seriesRef = useRef<Series>(newSeries());
  const t0Ref = useRef<number | null>(null);
  const chartHost = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<uPlot | null>(null);
  const [latest, setLatest] = useState(motor.latest);

  // Absorb freshest telemetry from the parent poll (query auto-refetches).
  useEffect(() => {
    if (!motor.latest) return;
    setLatest(motor.latest);
    const fb = motor.latest;
    const s = seriesRef.current;
    const tSec = Number(fb.t_ms) / 1000;
    if (t0Ref.current === null) t0Ref.current = tSec;
    const t = tSec - t0Ref.current;
    s.t.push(t);
    s.pos.push(fb.mech_pos_rad);
    s.vel.push(fb.mech_vel_rad_s);
    s.torque.push(fb.torque_nm);
    s.vbus.push(fb.vbus_v);
    // Drop samples older than WINDOW_SEC.
    while (s.t.length > 1 && s.t[s.t.length - 1] - s.t[0] > WINDOW_SEC) {
      s.t.shift();
      s.pos.shift();
      s.vel.shift();
      s.torque.shift();
      s.vbus.shift();
    }
    chartRef.current?.setData([s.t, s.pos, s.vel]);
  }, [motor.latest]);

  // Build the uPlot instance once.
  useEffect(() => {
    if (!chartHost.current) return;
    const opts: Options = {
      width: 500,
      height: 180,
      scales: { x: { time: false }, y: { auto: true } },
      axes: [
        { stroke: "#a1a1aa" },
        { stroke: "#a1a1aa" },
      ],
      series: [
        {},
        { label: "pos (rad)", stroke: "#60a5fa", width: 1.5 },
        { label: "vel (rad/s)", stroke: "#f59e0b", width: 1.5 },
      ],
    };
    const s = seriesRef.current;
    chartRef.current = new uPlot(opts, [s.t, s.pos, s.vel], chartHost.current);

    const ro = new ResizeObserver(() => {
      if (chartHost.current && chartRef.current) {
        chartRef.current.setSize({
          width: chartHost.current.clientWidth,
          height: 180,
        });
      }
    });
    ro.observe(chartHost.current);
    return () => {
      ro.disconnect();
      chartRef.current?.destroy();
      chartRef.current = null;
    };
  }, []);

  return (
    <div className="rounded-lg border border-border bg-card p-4">
      <div className="mb-2 flex items-baseline justify-between">
        <div>
          <div className="font-medium">{motor.role}</div>
          <div className="text-xs text-muted-foreground">
            can_id 0x{motor.can_id.toString(16).padStart(2, "0").toUpperCase()}{" "}
            on {motor.can_bus}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <span
            className={
              motor.verified
                ? "rounded-sm bg-emerald-500/10 px-1.5 py-0.5 text-xs text-emerald-400"
                : "rounded-sm bg-amber-500/10 px-1.5 py-0.5 text-xs text-amber-400"
            }
          >
            {motor.verified ? "verified" : "unverified"}
          </span>
        </div>
      </div>
      <div ref={chartHost} className="mb-2" />
      <dl className="grid grid-cols-4 gap-2 text-xs">
        <Stat label="pos" value={latest?.mech_pos_rad} unit="rad" />
        <Stat label="vel" value={latest?.mech_vel_rad_s} unit="rad/s" />
        <Stat label="vbus" value={latest?.vbus_v} unit="V" />
        <Stat label="temp" value={latest?.temp_c} unit="degC" />
      </dl>
    </div>
  );
}

function Stat({ label, value, unit }: { label: string; value: number | undefined; unit: string }) {
  return (
    <div className="rounded-md border border-border/60 bg-background px-2 py-1.5">
      <div className="text-muted-foreground">{label}</div>
      <div className="font-mono tabular-nums">
        {value === undefined ? "-" : value.toFixed(3)} {unit}
      </div>
    </div>
  );
}
