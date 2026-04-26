import { useEffect, useRef, useState } from "react";

/**
 * Samples `read()` when `flushKey` or `syncKey` change, then on a fixed timer.
 * Use for live numeric UI that should not re-render at wire / rAF rate.
 *
 * `read` should read from refs if parent props update every frame.
 */
export function useThrottledIntervalSnapshot<T>(
  read: () => T,
  intervalMs: number,
  flushKey: string | number,
  /**
   * When this value changes, `read()` runs immediately (e.g. `motor.latest != null`).
   */
  syncKey?: string | number | boolean,
): T {
  const readRef = useRef(read);
  readRef.current = read;
  const [state, setState] = useState<T>(() => read());

  useEffect(() => {
    setState(readRef.current());
  }, [flushKey, syncKey]);

  useEffect(() => {
    const id = window.setInterval(() => {
      setState(readRef.current());
    }, intervalMs);
    return () => clearInterval(id);
  }, [intervalMs, flushKey, syncKey]);

  return state;
}
