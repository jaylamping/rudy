// Hold-to-jog dead-man widget.
//
// While the operator holds a direction button the SPA drives a server-side
// jog through one of two transports:
//
//   1. WebTransport bidi stream (preferred). One stream per held press:
//      `MotionJog` on pointerDown, `MotionHeartbeat` every 200 ms while
//      held, `MotionStop` + close on release. Stream EOF without a
//      preceding stop is treated by the daemon as `ClientGone` and stops
//      the motor anyway, so a torn QUIC session can't leave the actuator
//      running.
//
//   2. REST fallback (`POST /api/motors/:role/motion/jog`). Same intent,
//      one HTTP request every 200 ms instead of a bidi frame. Used when
//      the WebTransport bridge isn't connected (config disabled, browser
//      lacks WT, transient drop). The daemon's heartbeat watchdog
//      (250 ms TTL) bounds the worst-case stop latency.
//
// Both paths share the same daemon-side controller (`motion::controller`)
// so the operator UX is identical: server runs the loop, browser only
// signals intent. See `crates/cortex/src/motion/mod.rs` for the
// architecture rule and the heartbeat constants.

import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Slider } from "@/components/ui/slider";
import { useLimbHealth } from "@/lib/hooks/useLimbHealth";
import { getBridgeWt } from "@/lib/hooks/wtBridgeHandle";
import { useWtConnected } from "@/lib/hooks/wtStatus";
import { Tooltip } from "@/components/ui/tooltip";
import type { ClientStreamHandle } from "@/lib/wt/clientStream";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import { degToRad, radToDeg } from "@/lib/units";

/**
 * Heartbeat cadence. Must be < `JOG_HEARTBEAT_TTL_MS` (250 ms) on the
 * daemon so a single dropped frame doesn't trip the watchdog. 200 ms
 * gives us 50 ms of slack and stays well under any reasonable HTTP
 * round-trip on the REST fallback path.
 */
const HEARTBEAT_INTERVAL_MS = 200;

/**
 * Max velocity the slider can request. Mirrored from the daemon's
 * `MAX_MOTION_VEL_RAD_S` so the slider can't ask for something that
 * silently clamps server-side.
 */
const MAX_VEL_RAD_S = 0.5;
const MAX_VEL_DEG_S = radToDeg(MAX_VEL_RAD_S);

type Transport = "wt" | "rest" | "unavailable";

export function DeadManJog({ motor }: { motor: MotorSummary }) {
  const [vel, setVel] = useState<[number]>([MAX_VEL_DEG_S]);
  const [error, setError] = useState<string | null>(null);
  const [available, setAvailable] = useState(true);
  const wtConnected = useWtConnected();
  const limb = useLimbHealth(motor.role);
  const jogBlocked = !limb.healthy;
  const jogDisableTip =
    jogBlocked && limb.blockReason
      ? limb.blockReason
      : !motor.verified
        ? "Jog requires a verified motor (Inventory tab)."
        : "";

  // Per-press state. We don't keep these in React state because the
  // pointer handlers run synchronously and a re-render in between
  // pointerDown and pointerUp could otherwise lose the stream handle.
  const directionRef = useRef<-1 | 0 | 1>(0);
  const timerRef = useRef<number | null>(null);
  const streamRef = useRef<ClientStreamHandle | null>(null);
  // Captures which transport this press started on so a mid-press
  // WebTransport disconnect doesn't try to send heartbeats on a dead
  // stream (we just let the daemon's TTL stop the motor).
  const transportRef = useRef<Transport>("unavailable");

  const stopJog = useCallback(() => {
    directionRef.current = 0;
    if (timerRef.current !== null) {
      window.clearInterval(timerRef.current);
      timerRef.current = null;
    }
    const stream = streamRef.current;
    streamRef.current = null;
    const wasTransport = transportRef.current;
    transportRef.current = "unavailable";

    if (wasTransport === "wt" && stream) {
      // Send MotionStop and close the stream. close() handles both even
      // if the stream is already half-broken.
      void stream.close(motor.role).catch(() => {
        // ignored: daemon will fall back to ClientGone-on-EOF
      });
    } else if (wasTransport === "rest") {
      // Best-effort explicit stop. The daemon's 250 ms heartbeat TTL
      // is the actual safety net; this just shaves the stop latency.
      void api.motion.stop(motor.role).catch(() => {
        // ignored
      });
    }
  }, [motor.role]);

  // Stop on unmount. The dependency array deliberately doesn't include
  // `vel` etc — we only want to stop when the component leaves.
  useEffect(() => () => stopJog(), [stopJog]);

  // If the WT bridge drops mid-press, don't try to keep heartbeating
  // on a dead stream — drop the press cleanly and let the daemon's
  // TTL handle it.
  useEffect(() => {
    if (transportRef.current === "wt" && !wtConnected) {
      stopJog();
    }
  }, [wtConnected, stopJog]);

  const startJog = (dir: -1 | 1) => {
    setError(null);
    directionRef.current = dir;
    if (timerRef.current !== null) return;

    const wt = wtConnected ? getBridgeWt() : null;

    void (async () => {
      // Pick a transport once per press. Mixing transports inside one
      // press would make the EOF/heartbeat semantics fight each other.
      let transport: Transport;
      let stream: ClientStreamHandle | null = null;
      if (wt) {
        try {
          stream = await wt.openClientStream();
          transport = "wt";
        } catch {
          // WT bridge claims connected but stream open failed — fall
          // through to REST. (Happens during the QUIC handshake race
          // right after a reconnect.)
          transport = "rest";
        }
      } else {
        transport = "rest";
      }

      // The user may have released the button while we were awaiting
      // the bidi handshake; bail before we send anything.
      if (directionRef.current === 0) {
        if (stream) void stream.close(motor.role).catch(() => {});
        return;
      }
      streamRef.current = stream;
      transportRef.current = transport;

      const sendOne = async (kind: "start" | "heartbeat") => {
        const d = directionRef.current;
        if (d === 0) return;
        const velRadS = d * degToRad(vel[0]);

        if (transport === "wt" && stream) {
          // For WT we send `MotionJog` on every tick rather than
          // distinguishing start vs. heartbeat: the daemon's
          // `dispatch_client_frame` treats a same-role re-jog as
          // an intent update (and refreshes the heartbeat for free),
          // and this means slider drags during a held press take
          // effect on the next tick without any extra plumbing.
          try {
            await stream.send({
              kind: "motion_jog",
              role: motor.role,
              vel_rad_s: velRadS,
            });
          } catch (e) {
            // Stream errored. Stop locally; daemon's TTL or the
            // EOF-as-ClientGone path will handle the motor.
            const msg = e instanceof Error ? e.message : String(e);
            setError(`WebTransport error: ${msg}`);
            stopJog();
          }
        } else {
          try {
            await api.motion.jog(motor.role, { vel_rad_s: velRadS });
          } catch (e) {
            if (e instanceof ApiError) {
              if (e.status === 404) {
                setAvailable(false);
                stopJog();
                return;
              }
              setError(e.message);
            } else {
              setError(String(e));
            }
            stopJog();
          }
        }
        // `kind` is reserved for future divergence (e.g. send a
        // dedicated MotionHeartbeat instead of MotionJog) but the
        // current daemon happily accepts repeated MotionJog frames.
        void kind;
      };

      await sendOne("start");
      // Use the wall-clock interval; a brief tab freeze stretches
      // the gap, the daemon's 250 ms TTL catches it, and we'll
      // re-jog on the next tick if the press is still held.
      timerRef.current = window.setInterval(() => {
        void sendOne("heartbeat");
      }, HEARTBEAT_INTERVAL_MS);
    })();
  };

  const transportLabel =
    wtConnected ? "WebTransport bidi stream" : "REST polling fallback";

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Hold-to-jog</CardTitle>
        <CardDescription>
          Press and hold a direction to jog. The daemon runs the velocity
          loop; the browser only signals intent. Releasing the button stops
          immediately; a frozen tab is bounded by the daemon's 250 ms
          heartbeat watchdog. Velocity is clamped to firmware limits
          server-side. Active transport: <span className="font-mono">{transportLabel}</span>.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="space-y-1.5">
          <div className="flex items-baseline justify-between text-xs text-muted-foreground">
            <span>Target velocity</span>
            <span className="font-mono tabular-nums text-foreground">
              {vel[0].toFixed(1)} °/s
            </span>
          </div>
          <Slider
            value={vel}
            min={0}
            max={MAX_VEL_DEG_S}
            step={1}
            onValueChange={(v) => setVel(v)}
            disabled={!available}
          />
        </div>
        <div className="flex items-center gap-2">
          {jogDisableTip ? (
            <Tooltip
              content={jogDisableTip}
              className="max-w-xs whitespace-normal"
            >
              <span className="flex flex-1">
                <Button
                  variant="outline"
                  size="lg"
                  disabled={!available || !motor.verified || jogBlocked}
                  onPointerDown={() => startJog(-1)}
                  onPointerUp={stopJog}
                  onPointerLeave={stopJog}
                  onPointerCancel={stopJog}
                  className="flex-1"
                >
                  <ChevronLeft className="h-5 w-5" /> Hold to jog -
                </Button>
              </span>
            </Tooltip>
          ) : (
            <Button
              variant="outline"
              size="lg"
              disabled={!available || !motor.verified || jogBlocked}
              onPointerDown={() => startJog(-1)}
              onPointerUp={stopJog}
              onPointerLeave={stopJog}
              onPointerCancel={stopJog}
              className="flex-1"
            >
              <ChevronLeft className="h-5 w-5" /> Hold to jog -
            </Button>
          )}
          {jogDisableTip ? (
            <Tooltip
              content={jogDisableTip}
              className="max-w-xs whitespace-normal"
            >
              <span className="flex flex-1">
                <Button
                  variant="outline"
                  size="lg"
                  disabled={!available || !motor.verified || jogBlocked}
                  onPointerDown={() => startJog(1)}
                  onPointerUp={stopJog}
                  onPointerLeave={stopJog}
                  onPointerCancel={stopJog}
                  className="flex-1"
                >
                  Hold to jog + <ChevronRight className="h-5 w-5" />
                </Button>
              </span>
            </Tooltip>
          ) : (
            <Button
              variant="outline"
              size="lg"
              disabled={!available || !motor.verified || jogBlocked}
              onPointerDown={() => startJog(1)}
              onPointerUp={stopJog}
              onPointerLeave={stopJog}
              onPointerCancel={stopJog}
              className="flex-1"
            >
              Hold to jog + <ChevronRight className="h-5 w-5" />
            </Button>
          )}
        </div>
        {!available && (
          <p className="text-xs text-amber-400">
            Motion endpoint is not yet deployed on this cortex build.
            Deploy a newer daemon or re-enable the operator-console build.
          </p>
        )}
        {jogBlocked && available && (
          <p className="text-xs text-amber-400">{limb.blockReason}</p>
        )}
        {!motor.verified && available && !jogBlocked && (
          <p className="text-xs text-amber-400">
            Jog requires a verified motor. Mark it verified from the
            Inventory tab.
          </p>
        )}
        {error && <p className="text-xs text-destructive">{error}</p>}
      </CardContent>
    </Card>
  );
}
