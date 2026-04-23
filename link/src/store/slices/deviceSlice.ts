import type { SliceCreator } from "../types";

/**
 * Liveness is driven by the motors query (WT-updated) + a wall-clock threshold.
 * `lastSeenMs` is `null` if we have never received a feedback frame.
 */
export type DeviceLiveness = {
  isOnline: boolean;
  lastSeenMs: number | null;
};

type DeviceState = {
  /** Keyed by motor `role` (API + URL). */
  devices: Record<string, DeviceLiveness>;
};

type DeviceActions = {
  setDeviceLiveness: (role: string, next: DeviceLiveness) => void;
  setManyDeviceLiveness: (next: Record<string, DeviceLiveness>) => void;
};

export type DeviceSlice = DeviceState & DeviceActions;

function livenessEqual(a: DeviceLiveness, b: DeviceLiveness) {
  return a.isOnline === b.isOnline && a.lastSeenMs === b.lastSeenMs;
}

function devicesMapEqual(
  a: Record<string, DeviceLiveness>,
  b: Record<string, DeviceLiveness>,
) {
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  if (aKeys.length !== bKeys.length) return false;
  for (const k of aKeys) {
    const x = a[k];
    const y = b[k];
    if (x == null && y == null) continue;
    if (x == null || y == null) return false;
    if (!livenessEqual(x, y)) return false;
  }
  return true;
}

const initial: DeviceState = { devices: {} };

export const createDeviceSlice: SliceCreator<DeviceSlice> = (set, get) => ({
  ...initial,

  setDeviceLiveness: (role, next) => {
    const cur = get().devices[role];
    if (cur && livenessEqual(cur, next)) return;
    set(
      (s) => ({ devices: { ...s.devices, [role]: next } }),
      false,
    );
  },

  setManyDeviceLiveness: (next) => {
    set(
      (s) => (devicesMapEqual(s.devices, next) ? s : { devices: next }),
      false,
    );
  },
});
