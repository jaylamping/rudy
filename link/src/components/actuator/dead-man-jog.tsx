// Hold-to-jog dead-man widget.
//
// While a button is held the SPA fires `POST /api/motors/:role/jog` at 20 Hz
// with a small velocity-mode setpoint and a TTL just longer than the
// inter-message gap. Releasing the button stops sending; the daemon's
// per-motor watchdog issues `cmd_stop` when the TTL lapses, so a dropped
// click never leaves the motor running.
//
// The endpoint is added by the daemon-side `jog_endpoint` task. Until that
// lands the widget renders disabled with a "not yet wired" hint, but the UI
// shape is the final one.

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
import type { MotorSummary } from "@/lib/types/MotorSummary";

const SEND_INTERVAL_MS = 50; // 20 Hz
const TTL_MS = 200; // > 2 send intervals

export function DeadManJog({ motor }: { motor: MotorSummary }) {
  const [vel, setVel] = useState<[number]>([0.5]); // rad/s
  const [error, setError] = useState<string | null>(null);
  const [available, setAvailable] = useState(true);
  const directionRef = useRef<-1 | 0 | 1>(0);
  const timerRef = useRef<number | null>(null);

  const stopJog = useCallback(() => {
    directionRef.current = 0;
    if (timerRef.current !== null) {
      window.clearInterval(timerRef.current);
      timerRef.current = null;
    }
    // Best-effort explicit stop (server's TTL watchdog will get us anyway).
    api.stop(motor.role).catch(() => {
      // ignored
    });
  }, [motor.role]);

  useEffect(() => () => stopJog(), [stopJog]);

  const startJog = (dir: -1 | 1) => {
    setError(null);
    directionRef.current = dir;
    if (timerRef.current !== null) return;
    const tick = async () => {
      const d = directionRef.current;
      if (d === 0) return;
      try {
        await api.jog(motor.role, { vel_rad_s: d * vel[0], ttl_ms: TTL_MS });
      } catch (e) {
        if (e instanceof ApiError) {
          if (e.status === 404) {
            // Endpoint not deployed yet; gracefully disable the controls so
            // the operator sees the "not yet wired" hint instead of a stack
            // of ApiError toasts.
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
    };
    void tick();
    timerRef.current = window.setInterval(
      () => void tick(),
      SEND_INTERVAL_MS,
    );
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Hold-to-jog</CardTitle>
        <CardDescription>
          Press and hold a direction to jog. Release immediately stops; if the
          UI freezes the daemon's TTL watchdog stops the motor in {TTL_MS} ms.
          Velocity is clamped to firmware limits server-side.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="space-y-1.5">
          <div className="flex items-baseline justify-between text-xs text-muted-foreground">
            <span>Target velocity</span>
            <span className="font-mono tabular-nums text-foreground">
              {vel[0].toFixed(2)} rad/s
            </span>
          </div>
          <Slider
            value={vel}
            min={0}
            max={2.0}
            step={0.05}
            onValueChange={(v) => setVel(v)}
            disabled={!available}
          />
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="lg"
            disabled={!available || !motor.verified}
            onPointerDown={() => startJog(-1)}
            onPointerUp={stopJog}
            onPointerLeave={stopJog}
            onPointerCancel={stopJog}
            className="flex-1"
          >
            <ChevronLeft className="h-5 w-5" /> Hold to jog -
          </Button>
          <Button
            variant="outline"
            size="lg"
            disabled={!available || !motor.verified}
            onPointerDown={() => startJog(1)}
            onPointerUp={stopJog}
            onPointerLeave={stopJog}
            onPointerCancel={stopJog}
            className="flex-1"
          >
            Hold to jog + <ChevronRight className="h-5 w-5" />
          </Button>
        </div>
        {!available && (
          <p className="text-xs text-amber-400">
            Jog endpoint is not yet deployed on this rudydae build. Deploy a
            newer daemon or re-enable the operator-console build.
          </p>
        )}
        {!motor.verified && available && (
          <p className="text-xs text-amber-400">
            Jog requires a verified motor. Mark it verified from the Inventory
            tab.
          </p>
        )}
        {error && <p className="text-xs text-destructive">{error}</p>}
      </CardContent>
    </Card>
  );
}
