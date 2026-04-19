// Motion-tests card.
//
// Closed-loop motion patterns layered on top of the existing `jog` endpoint.
// Each pattern runs as a 20 Hz client-side loop that:
//   - reads live mech_pos_rad from the motor's WT-fed cache,
//   - decides a target velocity for the next 50 ms window, and
//   - POSTs `/api/motors/:role/jog` with vel_rad_s + ttl_ms = 200.
//
// Stopping is multi-layered: clicking Stop, unmounting the tab, the motor
// going un-verified or out-of-band, or any jog error all cancel the loop
// and issue an explicit `api.stop`. As a final backstop the daemon's TTL
// watchdog fires `cmd_stop` ~200 ms after the last frame.
//
// The actuator's soft travel band remains the authoritative envelope; jog
// itself rejects projections outside the band, so the patterns here only
// need to be "polite" about turning around early.

import { useCallback, useEffect, useRef, useState } from "react";
import { Activity, Square, Waves } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Slider } from "@/components/ui/slider";
import { Label } from "@/components/ui/label";
import type { MotorSummary } from "@/lib/types/MotorSummary";

// 60 Hz mirror of the daemon's new poll cadence; matches the rAF
// frequency in modern browsers, so we stop coalescing pos updates
// behind a 50 ms heartbeat. See the Sweep-safe CAN I/O plan for why
// matching cadences end-to-end matters end-to-end.
const SEND_INTERVAL_MS = 16;
const TTL_MS = 100;
// Mirror of `safety.max_feedback_age_ms` (default 250 ms) on the daemon.
// Bail out client-side at the same threshold so the SPA stops sending
// before rudydae has to refuse the next jog with `stale_telemetry`.
//
// The threshold accounts for the type-17 fallback cadence on idle motors
// (~poll_interval_ms × motors_per_bus); 250 ms absorbs that worst case
// while still failing closed within ~15 type-2 frames if a sweep stalls
// mid-flight. Keep this in sync with `config.rs::default_max_feedback_age_ms`.
const MAX_FEEDBACK_AGE_MS = 250;
const RAD_TO_DEG = 180 / Math.PI;
const DEG_TO_RAD = Math.PI / 180;

// Daemon-side hard cap (see MAX_JOG_VEL_RAD_S in api/jog.rs). Mirrored
// here so the slider can't request something that would clamp on the wire.
const MAX_VEL_RAD_S = 0.5;

type PatternId = "wave" | "sweep";

interface PatternState {
  id: PatternId;
  // Wave: oscillate +/- amplitudeDeg around the position when the run started.
  centerRad: number;
  amplitudeRad: number;
  // Common: slew speed and a turnaround margin so we reverse just shy
  // of the soft target rather than slamming into the band edge.
  speedRadS: number;
  turnaroundRad: number;
  // Direction is updated each tick by the closed-loop controller.
  direction: -1 | 1;
}

export function MotionTestsCard({ motor }: { motor: MotorSummary }) {
  const qc = useQueryClient();
  const [waveAmpDeg, setWaveAmpDeg] = useState<[number]>([15]);
  const [waveSpeedRad, setWaveSpeedRad] = useState<[number]>([0.3]);
  const [sweepSpeedRad, setSweepSpeedRad] = useState<[number]>([0.25]);
  const [active, setActive] = useState<PatternId | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [available, setAvailable] = useState(true);

  const stateRef = useRef<PatternState | null>(null);
  const timerRef = useRef<number | null>(null);

  // Read the freshest position out of the WT-fed motors cache rather than
  // capturing it in a closure, so the loop sees ~200 Hz feedback updates.
  const livePosRad = useCallback((): number | null => {
    const motors = qc.getQueryData<MotorSummary[]>(["motors"]);
    const m = motors?.find((mm) => mm.role === motor.role);
    return m?.latest?.mech_pos_rad ?? null;
  }, [qc, motor.role]);

  // Companion to `livePosRad` — returns the cached feedback `t_ms` so the
  // tick loop can fail-closed on stalled telemetry, mirroring the daemon's
  // `safety.max_feedback_age_ms` guard. Returns `null` when no row exists
  // (treated as "stale" by callers).
  const liveFeedbackAgeMs = useCallback((): number | null => {
    const motors = qc.getQueryData<MotorSummary[]>(["motors"]);
    const m = motors?.find((mm) => mm.role === motor.role);
    const tMs = m?.latest?.t_ms;
    if (tMs == null) return null;
    return Date.now() - Number(tMs);
  }, [qc, motor.role]);

  const stopMotion = useCallback(
    (reason?: string) => {
      stateRef.current = null;
      if (timerRef.current !== null) {
        window.clearInterval(timerRef.current);
        timerRef.current = null;
      }
      setActive(null);
      if (reason) setError(reason);
      api.stop(motor.role).catch(() => {
        // ignored — TTL watchdog stops the motor regardless.
      });
    },
    [motor.role],
  );

  useEffect(() => () => stopMotion(), [stopMotion]);

  // Auto-stop if the safety preconditions disappear out from under us.
  useEffect(() => {
    if (active === null) return;
    if (!motor.verified) {
      stopMotion("Motor unverified; motion stopped.");
      return;
    }
    const bs = motor.boot_state.kind;
    if (bs === "out_of_band" || bs === "auto_recovering" || bs === "unknown") {
      stopMotion(`Boot state ${bs}; motion stopped.`);
    }
  }, [active, motor.verified, motor.boot_state.kind, stopMotion]);

  const startPattern = (id: PatternId) => {
    setError(null);
    const limits = motor.travel_limits;
    if (!limits) {
      setError("Configure travel limits before running motion patterns.");
      return;
    }
    const pos = livePosRad();
    if (pos == null) {
      setError("No live position telemetry yet.");
      return;
    }

    let st: PatternState;
    if (id === "wave") {
      const amplitudeRad = waveAmpDeg[0] * DEG_TO_RAD;
      // Clip the wave window so it can't poke past the band even if the
      // user picked a bigger amplitude than the band allows.
      const lo = Math.max(limits.min_rad + 0.01, pos - amplitudeRad);
      const hi = Math.min(limits.max_rad - 0.01, pos + amplitudeRad);
      if (hi - lo < 0.02) {
        setError(
          "Not enough headroom in the travel band for this wave amplitude.",
        );
        return;
      }
      const center = (lo + hi) / 2;
      st = {
        id,
        centerRad: center,
        amplitudeRad: (hi - lo) / 2,
        speedRadS: Math.min(waveSpeedRad[0], MAX_VEL_RAD_S),
        turnaroundRad: 0.02, // ~1.1 degrees
        direction: 1,
      };
    } else {
      // Sweep the full travel band. centerRad is unused; we drive against
      // the configured min/max with a small margin.
      st = {
        id,
        centerRad: (limits.min_rad + limits.max_rad) / 2,
        amplitudeRad: (limits.max_rad - limits.min_rad) / 2,
        speedRadS: Math.min(sweepSpeedRad[0], MAX_VEL_RAD_S),
        turnaroundRad: 0.05, // ~2.9 degrees from the band edge
        direction: pos > (limits.min_rad + limits.max_rad) / 2 ? -1 : 1,
      };
    }

    stateRef.current = st;
    setActive(id);

    const tick = async () => {
      const s = stateRef.current;
      if (s == null) return;
      const pos2 = livePosRad();
      if (pos2 == null) {
        stopMotion("Lost telemetry mid-run.");
        return;
      }
      // Mirror of the daemon's stale-feedback guard. If the cached feedback
      // is older than MAX_FEEDBACK_AGE_MS the next jog is going to be
      // refused with `stale_telemetry` anyway; bail proactively so we
      // surface "Telemetry stalled" instead of a 409 toast and stop sending
      // dead frames into the bus.
      const ageMs = liveFeedbackAgeMs();
      if (ageMs == null || ageMs > MAX_FEEDBACK_AGE_MS) {
        stopMotion("Telemetry stalled");
        return;
      }
      const lim = motor.travel_limits;
      if (!lim) {
        stopMotion("Travel limits cleared mid-run.");
        return;
      }

      // Pick the turnaround target for this pattern.
      let lo: number;
      let hi: number;
      if (s.id === "wave") {
        lo = s.centerRad - s.amplitudeRad;
        hi = s.centerRad + s.amplitudeRad;
      } else {
        lo = lim.min_rad + s.turnaroundRad;
        hi = lim.max_rad - s.turnaroundRad;
      }

      // Reverse direction if we've reached (or overshot) the active edge.
      if (s.direction > 0 && pos2 >= hi) s.direction = -1;
      else if (s.direction < 0 && pos2 <= lo) s.direction = 1;

      const v = s.direction * s.speedRadS;
      try {
        await api.jog(motor.role, { vel_rad_s: v, ttl_ms: TTL_MS });
      } catch (e) {
        if (e instanceof ApiError) {
          if (e.status === 404) {
            setAvailable(false);
            stopMotion();
            return;
          }
          stopMotion(e.message);
        } else {
          stopMotion(String(e));
        }
      }
    };
    void tick();
    timerRef.current = window.setInterval(() => void tick(), SEND_INTERVAL_MS);
  };

  const limits = motor.travel_limits;
  const liveDeg =
    motor.latest != null ? motor.latest.mech_pos_rad * RAD_TO_DEG : null;
  const bsKind = motor.boot_state.kind;
  const safetyReady =
    motor.verified &&
    !!limits &&
    bsKind !== "out_of_band" &&
    bsKind !== "auto_recovering" &&
    bsKind !== "unknown";

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Motion patterns</CardTitle>
        <CardDescription>
          Closed-loop motion routines layered on the jog endpoint. The daemon's
          travel-band check still vets every frame, and the TTL watchdog stops
          the motor within {TTL_MS} ms if anything stops sending. Use these to
          shake the joint out, demo the workspace, or sanity-check freshly
          edited limits.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex items-center justify-between rounded-md border border-border bg-background p-3 text-xs">
          <div className="flex items-center gap-3">
            {active ? (
              <Badge variant="warning" className="animate-pulse">
                running: {active}
              </Badge>
            ) : (
              <Badge variant="outline">idle</Badge>
            )}
            <span className="text-muted-foreground">
              live:{" "}
              <span className="font-mono text-foreground">
                {liveDeg == null ? "-" : `${liveDeg.toFixed(1)}°`}
              </span>
            </span>
          </div>
          {active && (
            <Button size="sm" variant="destructive" onClick={() => stopMotion()}>
              <Square className="h-3.5 w-3.5" /> Stop
            </Button>
          )}
        </div>

        <PatternRow
          icon={<Waves className="h-4 w-4" />}
          title="Wave"
          subtitle="Symmetric oscillation around the joint's current position. Good for limbering up a fresh actuator without sweeping the whole band."
          running={active === "wave"}
          disabled={!available || !safetyReady || (active !== null && active !== "wave")}
          onStart={() => startPattern("wave")}
          onStop={() => stopMotion()}
          controls={
            <div className="grid grid-cols-2 gap-3">
              <SliderRow
                label="Amplitude"
                unit="°"
                value={waveAmpDeg}
                min={2}
                max={45}
                step={1}
                onChange={setWaveAmpDeg}
                disabled={active === "wave"}
                fmt={(n) => n.toFixed(0)}
              />
              <SliderRow
                label="Speed"
                unit="rad/s"
                value={waveSpeedRad}
                min={0.05}
                max={MAX_VEL_RAD_S}
                step={0.05}
                onChange={setWaveSpeedRad}
                disabled={active === "wave"}
                fmt={(n) => n.toFixed(2)}
              />
            </div>
          }
        />

        <PatternRow
          icon={<Activity className="h-4 w-4" />}
          title="Sweep travel limits"
          subtitle={
            limits
              ? `Continuously travels between the configured min (${(limits.min_rad * RAD_TO_DEG).toFixed(1)}°) and max (${(limits.max_rad * RAD_TO_DEG).toFixed(1)}°), reversing just shy of each edge.`
              : "Configure soft travel limits above to enable this pattern."
          }
          running={active === "sweep"}
          disabled={!available || !safetyReady || (active !== null && active !== "sweep")}
          onStart={() => startPattern("sweep")}
          onStop={() => stopMotion()}
          controls={
            <SliderRow
              label="Speed"
              unit="rad/s"
              value={sweepSpeedRad}
              min={0.05}
              max={MAX_VEL_RAD_S}
              step={0.05}
              onChange={setSweepSpeedRad}
              disabled={active === "sweep"}
              fmt={(n) => n.toFixed(2)}
            />
          }
        />

        {!available && (
          <p className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-400">
            Jog endpoint is not yet deployed on this rudydae build; motion
            patterns are unavailable.
          </p>
        )}
        {available && !motor.verified && (
          <p className="text-xs text-amber-400">
            Motion patterns require a verified motor. Mark it verified from
            the Inventory tab.
          </p>
        )}
        {available && motor.verified && !limits && (
          <p className="text-xs text-amber-400">
            Save soft travel limits above before running motion patterns —
            they bound every commanded move.
          </p>
        )}
        {available && motor.verified && limits && !safetyReady && (
          <p className="text-xs text-amber-400">
            Boot state is <code className="font-mono">{bsKind}</code>; resolve
            it (Verify &amp; Home / wait for auto-recovery) before running
            motion.
          </p>
        )}
        {error && <p className="text-xs text-destructive">{error}</p>}
      </CardContent>
    </Card>
  );
}

function PatternRow({
  icon,
  title,
  subtitle,
  controls,
  running,
  disabled,
  onStart,
  onStop,
}: {
  icon: React.ReactNode;
  title: string;
  subtitle: string;
  controls: React.ReactNode;
  running: boolean;
  disabled: boolean;
  onStart: () => void;
  onStop: () => void;
}) {
  return (
    <div className="rounded-md border border-border/60 bg-background p-3">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-muted-foreground">{icon}</span>
            <span className="font-medium">{title}</span>
            {running && (
              <Badge variant="warning" className="animate-pulse">
                running
              </Badge>
            )}
          </div>
          <p className="mt-1 text-xs text-muted-foreground">{subtitle}</p>
        </div>
        {running ? (
          <Button variant="destructive" onClick={onStop}>
            <Square className="h-4 w-4" /> Stop
          </Button>
        ) : (
          <Button variant="default" disabled={disabled} onClick={onStart}>
            Start
          </Button>
        )}
      </div>
      <div className="mt-3">{controls}</div>
    </div>
  );
}

function SliderRow({
  label,
  unit,
  value,
  min,
  max,
  step,
  onChange,
  disabled,
  fmt,
}: {
  label: string;
  unit: string;
  value: [number];
  min: number;
  max: number;
  step: number;
  onChange: (v: [number]) => void;
  disabled: boolean;
  fmt: (n: number) => string;
}) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between text-xs">
        <Label className="text-xs text-muted-foreground">{label}</Label>
        <span className="font-mono tabular-nums text-foreground">
          {fmt(value[0])} {unit}
        </span>
      </div>
      <Slider
        value={value}
        min={min}
        max={max}
        step={step}
        onValueChange={(v) => onChange([v[0]] as [number])}
        disabled={disabled}
      />
    </div>
  );
}
