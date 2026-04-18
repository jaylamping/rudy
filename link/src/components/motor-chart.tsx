// Reusable per-motor uPlot chart driven by the cached `["motors"]` query.
//
// Originally inlined inside `<TelemetryGrid>`'s `<MotorCard>`; pulled out so
// the per-actuator detail page (`/_authed/actuators/:role`) can render one
// chart per metric without duplicating the rolling-window buffer.
//
// Rendering contract:
//   - one uPlot instance per mount, ResizeObserver wires width to the host
//   - 60 second rolling window, dropping oldest samples in place (avoids any
//     reallocation on the hot path)
//   - the bridge writes freshest-per-role telemetry into the shared cache;
//     this component reacts to `motor.latest` (the prop), so consumers must
//     pass a `MotorSummary` whose `.latest` updates over time

import { useEffect, useRef } from "react";
import uPlot, { type Options } from "uplot";
import "uplot/dist/uPlot.min.css";
import type { MotorSummary } from "@/lib/types/MotorSummary";

/** One of the four metrics we routinely chart per motor. */
export type MotorMetric = "pos" | "vel" | "torque" | "temp";

const WINDOW_SEC = 60;

interface SeriesBuffer {
  t: number[];
  v: number[];
}

const META: Record<
  MotorMetric,
  { label: string; unit: string; stroke: string }
> = {
  pos: { label: "pos (rad)", unit: "rad", stroke: "#60a5fa" },
  vel: { label: "vel (rad/s)", unit: "rad/s", stroke: "#f59e0b" },
  torque: { label: "torque (Nm)", unit: "Nm", stroke: "#a78bfa" },
  temp: { label: "temp (degC)", unit: "degC", stroke: "#f43f5e" },
};

function pluck(motor: MotorSummary | null | undefined, metric: MotorMetric): number | null {
  const fb = motor?.latest;
  if (!fb) return null;
  switch (metric) {
    case "pos":
      return fb.mech_pos_rad;
    case "vel":
      return fb.mech_vel_rad_s;
    case "torque":
      return fb.torque_nm;
    case "temp":
      return fb.temp_c;
  }
}

export interface MotorChartProps {
  motor: MotorSummary;
  metric: MotorMetric;
  /** Pixel height; defaults to 180 to match telemetry-grid. */
  height?: number;
}

export function MotorChart({ motor, metric, height = 180 }: MotorChartProps) {
  const meta = META[metric];
  const seriesRef = useRef<SeriesBuffer>({ t: [], v: [] });
  const t0Ref = useRef<number | null>(null);
  const chartHost = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<uPlot | null>(null);

  // Absorb one sample per `motor.latest` change. The bridge throttles updates
  // to ≤ 60 Hz via requestAnimationFrame so this can't melt the main thread
  // even on a 10-motor grid.
  useEffect(() => {
    if (!motor.latest) return;
    const fb = motor.latest;
    const v = pluck(motor, metric);
    if (v == null) return;

    const s = seriesRef.current;
    const tSec = Number(fb.t_ms) / 1000;
    if (t0Ref.current === null) t0Ref.current = tSec;
    s.t.push(tSec - t0Ref.current);
    s.v.push(v);

    while (s.t.length > 1 && s.t[s.t.length - 1] - s.t[0] > WINDOW_SEC) {
      s.t.shift();
      s.v.shift();
    }
    chartRef.current?.setData([s.t, s.v]);
  }, [motor, motor.latest, metric]);

  useEffect(() => {
    if (!chartHost.current) return;
    const opts: Options = {
      width: 500,
      height,
      scales: { x: { time: false }, y: { auto: true } },
      axes: [{ stroke: "#a1a1aa" }, { stroke: "#a1a1aa" }],
      series: [{}, { label: meta.label, stroke: meta.stroke, width: 1.5 }],
    };
    const s = seriesRef.current;
    chartRef.current = new uPlot(opts, [s.t, s.v], chartHost.current);

    const ro = new ResizeObserver(() => {
      if (chartHost.current && chartRef.current) {
        chartRef.current.setSize({
          width: chartHost.current.clientWidth,
          height,
        });
      }
    });
    ro.observe(chartHost.current);
    return () => {
      ro.disconnect();
      chartRef.current?.destroy();
      chartRef.current = null;
    };
    // We deliberately don't depend on `meta`/`metric`; remounting the
    // chart wipes the buffer. Callers should remount via `key={metric}`
    // if they want to switch metrics on a single host.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [height]);

  return <div ref={chartHost} />;
}
