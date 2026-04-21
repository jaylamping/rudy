// Motion-tests card.
//
// Closed-loop motion patterns are owned by the daemon (`crates/cortex/src/motion/`).
// This card is a pure intent + observe surface:
//
//   1. POST /api/motors/:role/motion/{sweep,wave} once to start.
//   2. Subscribe to the `motion_status` WebTransport stream filtered to
//      this role to render the live "running: <kind>" badge and the last
//      commanded velocity.
//   3. POST /api/motors/:role/motion/stop on Stop button / unmount.
//
// There is no per-frame loop in here. The browser does not drive the
// motor; the bus_worker on the Pi does. See the convention doc in
// `crates/cortex/src/motion/mod.rs` for the rationale.

import { useEffect, useRef, useState, type ReactNode } from "react";
import { Activity, Square, Waves } from "lucide-react";
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
import { useLimbHealth } from "@/lib/hooks/useLimbHealth";
import { getBridgeWt } from "@/lib/hooks/wtBridgeHandle";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import type { MotionStatus } from "@/lib/types/MotionStatus";
import {
  degToRad,
  formatAngularVelDeg,
  radToDeg,
} from "@/lib/units";

// Daemon-side hard cap (see MAX_PATTERN_VEL_RAD_S in api/motion.rs).
// Mirrored here so the slider can't request something that would clamp
// silently on the wire. The dead-man jog UI uses a tighter cap (0.5)
// because it's free-running; the bounded sweep/wave patterns can run
// faster safely because they self-reverse inside the travel band.
const MAX_VEL_RAD_S = 2.0;

// Mirrors `default_turnaround_rad` in `crates/cortex/src/motion/intent.rs`.
// Used only to render the operator-facing "reverses Xdeg before each
// edge at this speed" hint in the sweep subtitle — the daemon is the
// source of truth for the actual inset that gets baked into the intent.
const SWEEP_BASE_INSET_RAD = 0.05;
const OVERSHOOT_S = 0.15;
const sweepInsetRad = (speedRadS: number) =>
  SWEEP_BASE_INSET_RAD + Math.abs(speedRadS) * OVERSHOOT_S;

type PatternId = "wave" | "sweep";

/**
 * Last-seen status from the server-side controller. Carries the live
 * commanded velocity so the UI can show "running at +14.3°/s" without
 * re-deriving it from the pattern parameters.
 */
interface LiveStatus {
  kind: string;
  vel_rad_s: number;
  mech_pos_rad: number;
  state: "running" | "stopped";
  reason: string | null;
}

export function MotionTestsCard({ motor }: { motor: MotorSummary }) {
  const limb = useLimbHealth(motor.role);
  const [waveAmpDeg, setWaveAmpDeg] = useState<[number]>([15]);
  const [waveSpeedDeg, setWaveSpeedDeg] = useState<[number]>([
    radToDeg(0.3),
  ]);
  const [sweepSpeedDeg, setSweepSpeedDeg] = useState<[number]>([
    radToDeg(0.25),
  ]);
  const [active, setActive] = useState<PatternId | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [available, setAvailable] = useState(true);
  const [liveStatus, setLiveStatus] = useState<LiveStatus | null>(null);
  // Active run id; used to filter `motion_status` frames so a stale
  // datagram from a just-stopped run can't relight the badge.
  const runIdRef = useRef<string | null>(null);

  // Subscribe to the daemon's `motion_status` stream filtered to this
  // motor's role. The bridge owns the QUIC session; we just attach a
  // listener and a per-role filter narrowing.
  useEffect(() => {
    const wt = getBridgeWt();
    if (!wt) return;

    // Narrow the bridge subscription so only this role's motion_status
    // (plus the always-on telemetry kinds) reach this tab. Restored on
    // unmount so other tabs aren't permanently narrowed.
    void wt.setFilter({
      kinds: [
        "motor_feedback",
        "system_snapshot",
        "safety_event",
        "motion_status",
      ],
      filters: { motor_roles: [motor.role], run_ids: [] },
    });

    const off = wt.onKind<MotionStatus>("motion_status", (env) => {
      const ms = env.data;
      if (ms.role !== motor.role) return;
      // Drop frames from runs we don't own (a separate operator's run
      // for the same motor — should be rare; the registry permits one
      // controller per role).
      if (runIdRef.current && ms.run_id !== runIdRef.current) return;
      setLiveStatus({
        kind: ms.kind,
        vel_rad_s: ms.vel_rad_s,
        mech_pos_rad: ms.mech_pos_rad,
        state: ms.state,
        reason: ms.reason,
      });
      if (ms.state === "stopped") {
        // The daemon's terminal frame is the source of truth for
        // "we're idle now." Clear the badge regardless of which exit
        // path got us here (operator stop, heartbeat lapse, fault).
        runIdRef.current = null;
        setActive(null);
        if (ms.reason && ms.reason !== "operator") {
          setError(`Stopped: ${ms.reason}`);
        }
      }
    });

    return () => {
      off();
      void wt.setFilter({
        kinds: [],
        filters: { motor_roles: [], run_ids: [] },
      });
    };
  }, [motor.role]);

  // Reconcile against the GET snapshot on mount and on motor change so
  // the badge is correct before the WT stream catches up (and as the
  // recovery path if the terminal "stopped" datagram was dropped).
  useEffect(() => {
    let cancelled = false;
    api.motion
      .current(motor.role)
      .then((snap) => {
        if (cancelled) return;
        if (snap == null) {
          runIdRef.current = null;
          setActive(null);
        } else {
          runIdRef.current = snap.run_id;
          setActive(snap.kind === "wave" ? "wave" : snap.kind === "sweep" ? "sweep" : null);
        }
      })
      .catch((e) => {
        if (cancelled) return;
        if (e instanceof ApiError && e.status === 404) {
          setAvailable(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [motor.role]);

  const stopMotion = async (reason?: string) => {
    // Optimistic clear; the terminal MotionStatus datagram will
    // confirm. If the POST itself fails we leave the badge alone so
    // the operator notices.
    try {
      await api.motion.stop(motor.role);
      runIdRef.current = null;
      setActive(null);
      if (reason) setError(reason);
    } catch (e) {
      if (e instanceof ApiError) setError(e.message);
      else setError(String(e));
    }
  };

  // Cleanup: on unmount stop whatever's running for this role. The
  // controller's per-tick preflight + the daemon's safety paths cover
  // the case where the POST never lands (network drop / closed tab).
  useEffect(() => {
    return () => {
      if (runIdRef.current) {
        // Fire-and-forget; the unmount path can't await.
        void api.motion.stop(motor.role).catch(() => {});
      }
    };
  }, [motor.role]);

  // Auto-stop if the safety preconditions disappear out from under us.
  // The daemon also re-runs preflight every tick and will exit on its
  // own; this is a UI courtesy so the badge clears immediately rather
  // than waiting for the next status frame.
  useEffect(() => {
    if (active === null) return;
    if (!motor.verified) {
      void stopMotion("Motor unverified; motion stopped.");
      return;
    }
    const bs = motor.boot_state.kind;
    if (bs === "out_of_band" || bs === "auto_homing" || bs === "unknown") {
      void stopMotion(`Boot state ${bs}; motion stopped.`);
    }
  }, [active, motor.verified, motor.boot_state.kind, stopMotion]);

  const startPattern = async (id: PatternId) => {
    setError(null);
    setLiveStatus(null);
    const limits = motor.travel_limits;
    if (!limits) {
      setError("Configure travel limits before running motion patterns.");
      return;
    }

    try {
      if (id === "wave") {
        const amplitudeRad = degToRad(waveAmpDeg[0]);
        // Center the wave at the joint's current position; the daemon
        // clips against the band on every tick if the operator narrows
        // it mid-run.
        const pos = motor.latest?.mech_pos_rad;
        if (pos == null) {
          setError("No live position telemetry yet.");
          return;
        }
        const lo = Math.max(limits.min_rad + 0.01, pos - amplitudeRad);
        const hi = Math.min(limits.max_rad - 0.01, pos + amplitudeRad);
        if (hi - lo < 0.02) {
          setError(
            "Not enough headroom in the travel band for this wave amplitude.",
          );
          return;
        }
        const center = (lo + hi) / 2;
        const amp = (hi - lo) / 2;
        const speed = Math.min(degToRad(waveSpeedDeg[0]), MAX_VEL_RAD_S);
        const resp = await api.motion.wave(motor.role, {
          center_rad: center,
          amplitude_rad: amp,
          speed_rad_s: speed,
        });
        runIdRef.current = resp.run_id;
        setActive("wave");
      } else {
        const speed = Math.min(degToRad(sweepSpeedDeg[0]), MAX_VEL_RAD_S);
        const resp = await api.motion.sweep(motor.role, {
          speed_rad_s: speed,
        });
        runIdRef.current = resp.run_id;
        setActive("sweep");
      }
    } catch (e) {
      if (e instanceof ApiError) {
        if (e.status === 404) {
          setAvailable(false);
          return;
        }
        setError(e.message);
      } else {
        setError(String(e));
      }
    }
  };

  const limits = motor.travel_limits;
  const liveDeg =
    motor.latest != null ? radToDeg(motor.latest.mech_pos_rad) : null;
  const bsKind = motor.boot_state.kind;
  const safetyReady =
    motor.verified &&
    !!limits &&
    bsKind !== "out_of_band" &&
    bsKind !== "auto_homing" &&
    bsKind !== "unknown";

  const limbBlocked = !limb.healthy;
  const patternStartTip =
    limbBlocked && limb.blockReason ? limb.blockReason : undefined;

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Motion patterns</CardTitle>
        <CardDescription>
          Closed-loop motion runs entirely on the daemon: the browser POSTs
          a single intent, watches the live status stream, and POSTs once
          more to stop. The travel band, stale-telemetry guard, and
          per-tick preflight inside the controller bound every commanded
          frame.
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
            {liveStatus && active && (
              <span className="text-muted-foreground">
                cmd:{" "}
                <span className="font-mono text-foreground">
                  {formatAngularVelDeg(liveStatus.vel_rad_s, 2)}
                </span>
              </span>
            )}
          </div>
          {active && (
            <Button
              size="sm"
              variant="destructive"
              onClick={() => void stopMotion()}
            >
              <Square className="h-3.5 w-3.5" /> Stop
            </Button>
          )}
        </div>

        <PatternRow
          icon={<Waves className="h-4 w-4" />}
          title="Wave"
          subtitle="Symmetric oscillation around the joint's current position. Good for limbering up a fresh actuator without sweeping the whole band."
          running={active === "wave"}
          disabled={
            !available ||
            !safetyReady ||
            limbBlocked ||
            (active !== null && active !== "wave")
          }
          startTooltip={patternStartTip}
          onStart={() => void startPattern("wave")}
          onStop={() => void stopMotion()}
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
                unit="°/s"
                value={waveSpeedDeg}
                min={radToDeg(0.05)}
                max={radToDeg(MAX_VEL_RAD_S)}
                step={1}
                onChange={setWaveSpeedDeg}
                disabled={active === "wave"}
                fmt={(n) => n.toFixed(1)}
              />
            </div>
          }
        />

        <PatternRow
          icon={<Activity className="h-4 w-4" />}
          title="Sweep travel limits"
          subtitle={
            limits
              ? `Continuously travels between min (${radToDeg(limits.min_rad).toFixed(1)}°) and max (${radToDeg(limits.max_rad).toFixed(1)}°), reversing ~${radToDeg(sweepInsetRad(degToRad(sweepSpeedDeg[0]))).toFixed(1)}° before each edge to absorb motor overshoot at this speed.`
              : "Configure soft travel limits above to enable this pattern."
          }
          running={active === "sweep"}
          disabled={
            !available ||
            !safetyReady ||
            limbBlocked ||
            (active !== null && active !== "sweep")
          }
          startTooltip={patternStartTip}
          onStart={() => void startPattern("sweep")}
          onStop={() => void stopMotion()}
          controls={
            <SliderRow
              label="Speed"
              unit="°/s"
              value={sweepSpeedDeg}
              min={radToDeg(0.05)}
              max={radToDeg(MAX_VEL_RAD_S)}
              step={1}
              onChange={setSweepSpeedDeg}
              disabled={active === "sweep"}
              fmt={(n) => n.toFixed(1)}
            />
          }
        />

        {!available && (
          <p className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-400">
            Motion endpoint is not yet deployed on this cortex build;
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
        {available && motor.verified && limits && limbBlocked && (
          <p className="text-xs text-amber-400">{limb.blockReason}</p>
        )}
        {available && motor.verified && limits && !safetyReady && !limbBlocked && (
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
  startTooltip,
  onStart,
  onStop,
}: {
  icon: ReactNode;
  title: string;
  subtitle: string;
  controls: ReactNode;
  running: boolean;
  disabled: boolean;
  /** Shown when Start is disabled (e.g. limb quarantine). */
  startTooltip?: string;
  onStart: () => void;
  onStop: () => void;
}) {
  const startButton = (
    <Button variant="default" disabled={disabled} onClick={onStart}>
      Start
    </Button>
  );

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
        ) : startTooltip && disabled ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex">{startButton}</span>
            </TooltipTrigger>
            <TooltipContent className="max-w-xs whitespace-normal">
              {startTooltip}
            </TooltipContent>
          </Tooltip>
        ) : (
          startButton
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
