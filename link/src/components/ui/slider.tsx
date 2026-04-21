// Single-thumb numeric slider with min/max/step. Hand-rolled (mouse + touch)
// because the existing radix-ui dep doesn't expose its Slider primitive
// types directly and we don't need the dual-thumb / horizontal+vertical
// support shadcn's full slider provides.
//
// API matches shadcn's Slider closely:
//   <Slider value={[v]} onValueChange={([v]) => ...}
//           min={0} max={100} step={1} disabled />

import { forwardRef, useEffect, useRef } from "react";
import { cn } from "@/lib/utils";

export interface SliderProps
  extends Omit<
    React.HTMLAttributes<HTMLDivElement>,
    "defaultValue" | "onChange"
  > {
  value: [number];
  onValueChange?: (value: [number]) => void;
  onValueCommit?: (value: [number]) => void;
  min?: number;
  max?: number;
  step?: number;
  disabled?: boolean;
}

export const Slider = forwardRef<HTMLDivElement, SliderProps>(
  (
    {
      value,
      onValueChange,
      onValueCommit,
      min = 0,
      max = 100,
      step = 1,
      disabled = false,
      className,
      ...props
    },
    ref,
  ) => {
    const trackRef = useRef<HTMLDivElement | null>(null);
    const v = value[0];
    const pct = max === min ? 0 : ((v - min) / (max - min)) * 100;
    const lastCommittedRef = useRef<number>(v);

    const commit = (n: number) => {
      if (onValueCommit && lastCommittedRef.current !== n) {
        lastCommittedRef.current = n;
        onValueCommit([n]);
      }
    };

    const fromClientX = (clientX: number): number => {
      const track = trackRef.current;
      if (!track) return v;
      const rect = track.getBoundingClientRect();
      const ratio = Math.min(
        1,
        Math.max(0, (clientX - rect.left) / rect.width),
      );
      const raw = min + ratio * (max - min);
      const stepped = Math.round(raw / step) * step;
      return clamp(stepped, min, max);
    };

    const onPointer = (e: React.PointerEvent<HTMLDivElement>) => {
      if (disabled) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      const next = fromClientX(e.clientX);
      if (next !== v) onValueChange?.([next]);
    };

    const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
      if (disabled) return;
      if (!(e.buttons & 1)) return;
      const next = fromClientX(e.clientX);
      if (next !== v) onValueChange?.([next]);
    };

    const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
      if (disabled) return;
      e.currentTarget.releasePointerCapture(e.pointerId);
      commit(v);
    };

    const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (disabled) return;
      let delta = 0;
      if (e.key === "ArrowRight" || e.key === "ArrowUp") delta = step;
      else if (e.key === "ArrowLeft" || e.key === "ArrowDown") delta = -step;
      else if (e.key === "PageUp") delta = step * 10;
      else if (e.key === "PageDown") delta = -step * 10;
      else if (e.key === "Home") {
        onValueChange?.([min]);
        commit(min);
        e.preventDefault();
        return;
      } else if (e.key === "End") {
        onValueChange?.([max]);
        commit(max);
        e.preventDefault();
        return;
      }
      if (delta !== 0) {
        const next = clamp(v + delta, min, max);
        if (next !== v) onValueChange?.([next]);
        commit(next);
        e.preventDefault();
      }
    };

    useEffect(() => {
      lastCommittedRef.current = v;
    }, [v]);

    return (
      <div
        ref={ref}
        className={cn(
          "relative flex w-full touch-none select-none items-center",
          disabled && "opacity-50",
          className,
        )}
        {...props}
      >
        <div
          ref={trackRef}
          className="relative h-2 w-full grow overflow-hidden rounded-full bg-muted"
          onPointerDown={onPointer}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
        >
          <div
            className="absolute h-full bg-primary"
            style={{ width: `${pct}%` }}
          />
        </div>
        <div
          role="slider"
          aria-valuemin={min}
          aria-valuemax={max}
          aria-valuenow={v}
          aria-disabled={disabled || undefined}
          tabIndex={disabled ? -1 : 0}
          onKeyDown={onKey}
          className={cn(
            "absolute h-4 w-4 -translate-x-1/2 rounded-full border border-primary/50 bg-background shadow ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
            disabled && "cursor-not-allowed",
          )}
          style={{ left: `${pct}%` }}
        />
      </div>
    );
  },
);
Slider.displayName = "Slider";

function clamp(n: number, lo: number, hi: number) {
  if (n < lo) return lo;
  if (n > hi) return hi;
  return n;
}
