// Travel-limits tab.
//
// Per-actuator soft min/max position in radians (rendered in degrees). The
// daemon enforces these on every commanded move. Storage is the new
// `travel_limits: { min_rad, max_rad, updated_at }` field on each motor in
// `config/actuators/inventory.yaml`; PUT roundtrips that file atomically.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { ConfirmDialog } from "@/components/params";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import type { TravelLimits } from "@/lib/types/TravelLimits";

// Reasonable outer rail when the daemon hasn't been told a tighter cap
// (matches the RS03 spec.protocol.position_min/max_rad, which is +/- 2 turns).
const RAIL_DEG = 360;

const RAD_TO_DEG = 180 / Math.PI;
const DEG_TO_RAD = Math.PI / 180;

export function ActuatorTravelTab({ motor }: { motor: MotorSummary }) {
  const qc = useQueryClient();
  const limitsQ = useQuery({
    queryKey: ["travel_limits", motor.role],
    queryFn: () => api.getTravelLimits(motor.role),
    retry: false,
  });
  const supported = !(
    limitsQ.isError &&
    (limitsQ.error as ApiError | undefined)?.status === 404
  );

  const baseline: TravelLimits | null = limitsQ.data ?? motor.travel_limits ?? null;
  const [minDeg, setMinDeg] = useState<number>(toDeg(baseline?.min_rad ?? -Math.PI / 3));
  const [maxDeg, setMaxDeg] = useState<number>(toDeg(baseline?.max_rad ?? Math.PI / 3));
  const [confirm, setConfirm] = useState(false);

  // Re-baseline when the server-side data swaps in (or motor changes).
  useEffect(() => {
    if (baseline) {
      setMinDeg(toDeg(baseline.min_rad));
      setMaxDeg(toDeg(baseline.max_rad));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [motor.role, baseline?.min_rad, baseline?.max_rad]);

  const save = useMutation({
    mutationFn: () =>
      api.setTravelLimits(motor.role, {
        min_rad: minDeg * DEG_TO_RAD,
        max_rad: maxDeg * DEG_TO_RAD,
      }),
    onSuccess: () => {
      setConfirm(false);
      qc.invalidateQueries({ queryKey: ["travel_limits", motor.role] });
      qc.invalidateQueries({ queryKey: ["motors"] });
    },
  });

  if (limitsQ.isPending) {
    return <div className="text-muted-foreground">Loading travel limits...</div>;
  }

  // Live position read for the current-position marker on the band display.
  const liveDeg = motor.latest ? motor.latest.mech_pos_rad * RAD_TO_DEG : null;
  const dirty =
    Math.abs(minDeg - toDeg(baseline?.min_rad ?? 0)) > 1e-6 ||
    Math.abs(maxDeg - toDeg(baseline?.max_rad ?? 0)) > 1e-6;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Soft travel limits</CardTitle>
          <CardDescription>
            Per-actuator software band that the daemon enforces on every
            commanded move. The motor's firmware-level limits remain the
            authoritative envelope; this band is a tighter, easily-reversible
            inner cap. Stored in inventory.yaml, audited per change.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          {!supported && (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-400">
              Travel-limits endpoint is not yet deployed on this rudydae
              build. Deploy a newer daemon to enable saving.
            </p>
          )}
          <LimitRow
            label="Minimum"
            valueDeg={minDeg}
            min={-RAIL_DEG}
            max={RAIL_DEG}
            onChange={setMinDeg}
            disabled={!supported || save.isPending}
          />
          <LimitRow
            label="Maximum"
            valueDeg={maxDeg}
            min={-RAIL_DEG}
            max={RAIL_DEG}
            onChange={setMaxDeg}
            disabled={!supported || save.isPending}
          />

          <div className="rounded-md border border-border bg-background p-3 text-xs">
            <div className="mb-1 flex items-baseline justify-between">
              <span className="text-muted-foreground">Allowed band</span>
              <span className="font-mono tabular-nums">
                {minDeg.toFixed(1)}° - {maxDeg.toFixed(1)}°
              </span>
            </div>
            <BandStrip
              minDeg={minDeg}
              maxDeg={maxDeg}
              railDeg={RAIL_DEG}
              currentDeg={liveDeg}
            />
            {liveDeg != null && (
              <div className="mt-1 text-right text-muted-foreground">
                live: <span className="font-mono">{liveDeg.toFixed(1)}°</span>
              </div>
            )}
          </div>

          <div className="flex flex-wrap items-center justify-end gap-2">
            <Button
              variant="ghost"
              disabled={!dirty || save.isPending}
              onClick={() => {
                if (baseline) {
                  setMinDeg(toDeg(baseline.min_rad));
                  setMaxDeg(toDeg(baseline.max_rad));
                }
              }}
            >
              Reset
            </Button>
            <Button
              variant="default"
              disabled={!supported || !dirty || maxDeg <= minDeg || save.isPending}
              onClick={() => setConfirm(true)}
            >
              {save.isPending ? "Saving..." : "Save travel limits"}
            </Button>
          </div>

          {save.isError && (
            <p className="text-xs text-destructive">
              {(save.error as ApiError).message}
            </p>
          )}
          {maxDeg <= minDeg && (
            <p className="text-xs text-destructive">
              Maximum must be strictly greater than minimum.
            </p>
          )}
        </CardContent>
      </Card>

      {confirm && (
        <ConfirmDialog
          title="Save travel limits"
          description={
            <>
              Set <code className="font-mono">{motor.role}</code> travel band
              to{" "}
              <code className="font-mono">
                [{minDeg.toFixed(2)}°, {maxDeg.toFixed(2)}°]
              </code>
              . Any commanded position outside this band will be refused by
              rudydae.
            </>
          }
          phrase={`limit ${motor.role}`}
          confirmLabel="Save"
          confirmVariant="default"
          onCancel={() => setConfirm(false)}
          onConfirm={() => save.mutate()}
        />
      )}
    </div>
  );
}

function LimitRow({
  label,
  valueDeg,
  min,
  max,
  onChange,
  disabled,
}: {
  label: string;
  valueDeg: number;
  min: number;
  max: number;
  onChange: (n: number) => void;
  disabled: boolean;
}) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between gap-2">
        <Label className="text-sm">{label}</Label>
        <div className="flex items-center gap-1">
          <Input
            className="w-24 text-right font-mono"
            type="number"
            step={1}
            min={min}
            max={max}
            value={Number.isFinite(valueDeg) ? valueDeg.toFixed(2) : ""}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (Number.isFinite(n)) onChange(clamp(n, min, max));
            }}
            disabled={disabled}
          />
          <span className="text-xs text-muted-foreground">°</span>
        </div>
      </div>
      <Slider
        value={[valueDeg]}
        min={min}
        max={max}
        step={0.5}
        onValueChange={([v]) => onChange(v)}
        disabled={disabled}
      />
      <div className="flex justify-between text-[10px] text-muted-foreground">
        <span>{min}°</span>
        <span>{max}°</span>
      </div>
    </div>
  );
}

function BandStrip({
  minDeg,
  maxDeg,
  railDeg,
  currentDeg,
}: {
  minDeg: number;
  maxDeg: number;
  railDeg: number;
  currentDeg: number | null;
}) {
  const total = railDeg * 2;
  const leftPct = ((minDeg + railDeg) / total) * 100;
  const widthPct = ((maxDeg - minDeg) / total) * 100;
  const livePct =
    currentDeg == null
      ? null
      : ((clamp(currentDeg, -railDeg, railDeg) + railDeg) / total) * 100;
  return (
    <div className="relative h-3 w-full overflow-hidden rounded-full bg-muted">
      <div
        className="absolute h-full bg-emerald-500/30"
        style={{ left: `${leftPct}%`, width: `${widthPct}%` }}
      />
      {livePct !== null && (
        <div
          className="absolute top-0 h-full w-0.5 bg-foreground"
          style={{ left: `${livePct}%` }}
        />
      )}
    </div>
  );
}

function toDeg(rad: number) {
  return rad * RAD_TO_DEG;
}

function clamp(n: number, lo: number, hi: number) {
  if (n < lo) return lo;
  if (n > hi) return hi;
  return n;
}
