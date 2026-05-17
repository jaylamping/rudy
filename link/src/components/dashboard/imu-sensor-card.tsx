// Live BNO085 card. REST seeds queryKeys.sensors.all(); WebTransport patches
// newest sensor_sample frames into the same cache.

import { useQuery } from "@tanstack/react-query";
import { queryKeys } from "@/api";
import { api } from "@/lib/api";
import { useLiveInterval } from "@/lib/hooks/useLiveInterval";
import { cn } from "@/lib/utils";
import type { SensorHealth } from "@/lib/types/SensorHealth";
import type { ImuSample } from "@/lib/types/ImuSample";
import { DashboardCard } from "./dashboard-card";

const RAD_TO_DEG = 180 / Math.PI;

export function ImuSensorCard({ className }: { className?: string }) {
  const q = useQuery({
    queryKey: queryKeys.sensors.all(),
    queryFn: () => api.listSensors(),
    refetchInterval: useLiveInterval({ live: 30_000, fallback: 1_000 }),
  });

  const sensor = (q.data ?? []).find((s) => s.kind === "imu") ?? q.data?.[0];
  const imu = sensor?.imu;
  const rpy = imu ? quatToEulerDeg(imu.quaternion_xyzw) : null;
  const ageMs = sensor ? Math.max(0, Date.now() - Number(sensor.t_ms)) : null;

  return (
    <DashboardCard
      title="IMU"
      className={className}
      hint={
        sensor ? (
          <span className={cn("rounded-sm px-1.5 py-0.5", healthClass(sensor.health))}>
            {sensor.health}
          </span>
        ) : undefined
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
      {!q.isPending && !q.isError && !sensor && (
        <div className="text-sm text-muted-foreground">No sensors configured.</div>
      )}
      {sensor && (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <span className="font-mono text-foreground">{sensor.sensor_id}</span>
            <span>frame {sensor.frame_id}</span>
            {ageMs != null && <span>age {formatMs(ageMs)}</span>}
            {sensor.message && (
              <span className="text-amber-400">{sensor.message}</span>
            )}
          </div>

          {imu && rpy ? (
            <>
              <div className="grid grid-cols-3 gap-2">
                <Metric label="roll" value={rpy.roll} unit="deg" />
                <Metric label="pitch" value={rpy.pitch} unit="deg" />
                <Metric label="yaw" value={rpy.yaw} unit="deg" />
              </div>
              <div className="grid grid-cols-1 gap-2 text-xs sm:grid-cols-3">
                <Vector label="accel" values={imu.accel_m_s2} unit="m/s2" />
                <Vector label="gyro" values={imu.gyro_rad_s} unit="rad/s" />
                <Vector label="quat" values={imu.quaternion_xyzw} unit="xyzw" />
              </div>
              <div className="text-xs text-muted-foreground">
                rotation accuracy:{" "}
                <span className="font-mono text-foreground">
                  {imu.rotation_accuracy_label} ({imu.rotation_accuracy})
                </span>
              </div>
            </>
          ) : (
            <div className="rounded-md border border-border/60 bg-background px-3 py-2 text-sm text-muted-foreground">
              Waiting for IMU sample.
            </div>
          )}
        </div>
      )}
    </DashboardCard>
  );
}

function Metric({ label, value, unit }: { label: string; value: number; unit: string }) {
  return (
    <div className="rounded-md border border-border/60 bg-background px-2 py-1.5">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="font-mono text-sm tabular-nums">
        {value.toFixed(2)} {unit}
      </div>
    </div>
  );
}

function Vector({
  label,
  values,
  unit,
}: {
  label: string;
  values: number[] | readonly number[];
  unit: string;
}) {
  return (
    <div className="rounded-md border border-border/60 bg-background px-2 py-1.5">
      <div className="mb-1 text-muted-foreground">
        {label} <span className="text-[10px]">{unit}</span>
      </div>
      <div className="space-y-0.5 font-mono tabular-nums">
        {values.map((v, idx) => (
          <div key={idx}>{Number(v).toFixed(3)}</div>
        ))}
      </div>
    </div>
  );
}

function quatToEulerDeg(q: ImuSample["quaternion_xyzw"]) {
  const [x, y, z, w] = q;
  const sinrCosp = 2 * (w * x + y * z);
  const cosrCosp = 1 - 2 * (x * x + y * y);
  const roll = Math.atan2(sinrCosp, cosrCosp);

  const sinp = 2 * (w * y - z * x);
  const pitch =
    Math.abs(sinp) >= 1 ? Math.sign(sinp) * (Math.PI / 2) : Math.asin(sinp);

  const sinyCosp = 2 * (w * z + x * y);
  const cosyCosp = 1 - 2 * (y * y + z * z);
  const yaw = Math.atan2(sinyCosp, cosyCosp);

  return {
    roll: roll * RAD_TO_DEG,
    pitch: pitch * RAD_TO_DEG,
    yaw: yaw * RAD_TO_DEG,
  };
}

function formatMs(ms: number): string {
  if (ms < 1_000) return `${Math.round(ms)} ms`;
  return `${(ms / 1_000).toFixed(1)} s`;
}

function healthClass(health: SensorHealth): string {
  switch (health) {
    case "ok":
      return "bg-emerald-500/10 text-emerald-400";
    case "stale":
      return "bg-amber-500/10 text-amber-400";
    case "error":
    case "unavailable":
      return "bg-rose-500/10 text-rose-400";
    default:
      return "bg-muted text-muted-foreground";
  }
}
