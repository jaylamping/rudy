// Human-readable hints for RS03 fault_sta / warn_sta bits shown in the UI.
// Canonical protocol notes: docs/decisions/0002-rs03-protocol-spec.md

import type { MotorSummary } from "@/lib/types/MotorSummary";

const WARN_BIT_LABEL: Record<number, string> = {
  0: "Motor overtemperature warning — reduce load or improve cooling; cortex treats this as fatal to motion by default.",
  5: "Bit 5 (0x20) — common advisory on FW 0.3.1.41 (often uncogged / cogging cal); cortex ignores this bit alone for motion gating.",
};

const FAULT_BIT_LABEL: Record<number, string> = {
  5: "Bit 5 (0x20) — RS03 can keep this latched after Motor Studio recovery; try Controls → Clear fault, or E-stop then clear.",
};

function linesForBits(word: number, labels: Record<number, string>, kind: "fault" | "warn"): string[] {
  const out: string[] = [];
  for (let b = 0; b < 32; b++) {
    const mask = 1 << b;
    if ((word & mask) === 0) continue;
    out.push(
      labels[b] ??
        `Bit ${b} — not decoded in-app; check RobStride Motor Studio or manual §3.3.7 (${kind}Sta).`,
    );
  }
  return out;
}

export function describeFaultBits(faultSta: number): string[] {
  return linesForBits(faultSta, FAULT_BIT_LABEL, "fault");
}

export function describeWarnBits(warnSta: number): string[] {
  return linesForBits(warnSta, WARN_BIT_LABEL, "warn");
}

export function motorsWithFaultNonzero(motors: MotorSummary[]): MotorSummary[] {
  return motors.filter((m) => m.latest != null && m.latest.fault_sta !== 0);
}

/** Same criterion as dashboard `getTone` === "warn". */
export function motorsWithWarnOnly(motors: MotorSummary[]): MotorSummary[] {
  return motors.filter((m) => {
    const fb = m.latest;
    if (!fb) return false;
    return fb.fault_sta === 0 && fb.warn_sta !== 0;
  });
}

function hex32(n: number): string {
  return `0x${(n >>> 0).toString(16).padStart(8, "0")}`;
}

/** Multi-line plain text for TooltipContent (`whitespace-pre-line`). */
export function formatFaultRollup(motors: MotorSummary[]): string {
  const lines: string[] = ["Actuators reporting fault_sta ≠ 0:"];
  if (motors.length === 0) {
    lines.push("(No motors matched — try refreshing the page.)");
    return lines.join("\n");
  }
  for (const m of motors) {
    const fb = m.latest;
    if (!fb) continue;
    lines.push("");
    lines.push(`• ${m.role}`);
    lines.push(`  fault_sta=${hex32(fb.fault_sta)}  warn_sta=${hex32(fb.warn_sta)}`);
    for (const hint of describeFaultBits(fb.fault_sta)) {
      lines.push(`  ‣ ${hint}`);
    }
  }
  lines.push("");
  lines.push(
    "Next steps: open the actuator → Controls → Clear fault (or per-motor Stop), then re-home if needed. Full raw fields on Telemetry.",
  );
  return lines.join("\n");
}

export function formatWarnRollup(motors: MotorSummary[]): string {
  const lines: string[] = ["Actuators with warnings (fault_sta = 0, warn_sta ≠ 0):"];
  if (motors.length === 0) {
    lines.push("(No motors matched — try refreshing the page.)");
    return lines.join("\n");
  }
  for (const m of motors) {
    const fb = m.latest;
    if (!fb) continue;
    lines.push("");
    lines.push(`• ${m.role}`);
    lines.push(`  warn_sta=${hex32(fb.warn_sta)}`);
    for (const hint of describeWarnBits(fb.warn_sta)) {
      lines.push(`  ‣ ${hint}`);
    }
  }
  lines.push("");
  lines.push("If motion is blocked, address fatal warning bits first; see Settings → safety.fatal_warn_mask.");
  return lines.join("\n");
}
