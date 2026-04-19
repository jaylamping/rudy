import type { LimbQuarantineMotor } from "@/lib/types/LimbQuarantineMotor";

export type LimbQuarantineEvent = {
  limb: string;
  failedMotors: LimbQuarantineMotor[];
  detail: string | null;
};

type Listener = (ev: LimbQuarantineEvent) => void;

const listeners = new Set<Listener>();

export function subscribeLimbQuarantine(cb: Listener): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Called from `apiFetch` when the daemon returns a `limb_quarantined` envelope. */
export function emitLimbQuarantineFromWire(body: unknown): void {
  if (!body || typeof body !== "object") return;
  const rec = body as Record<string, unknown>;
  if (rec.error !== "limb_quarantined") return;
  const limb = typeof rec.limb === "string" ? rec.limb : "";
  const detail = typeof rec.detail === "string" ? rec.detail : null;
  const raw = rec.failed_motors;
  const failedMotors: LimbQuarantineMotor[] = [];
  if (Array.isArray(raw)) {
    for (const item of raw) {
      if (!item || typeof item !== "object") continue;
      const o = item as Record<string, unknown>;
      if (typeof o.role !== "string" || typeof o.state_kind !== "string") continue;
      failedMotors.push({ role: o.role, state_kind: o.state_kind });
    }
  }
  const ev: LimbQuarantineEvent = { limb, failedMotors, detail };
  for (const cb of listeners) {
    try {
      cb(ev);
    } catch {
      /* ignore subscriber errors */
    }
  }
}
