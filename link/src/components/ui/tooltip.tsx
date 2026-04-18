// Tiny CSS-driven tooltip. Renders the trigger inline; on hover/focus the
// content fades in below it. No portal — that keeps it dependency-free and
// good enough for the few inline help labels we render in the operator
// console.

import { type ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface TooltipProps {
  content: ReactNode;
  children: ReactNode;
  className?: string;
  side?: "top" | "bottom";
}

export function Tooltip({
  content,
  children,
  className,
  side = "top",
}: TooltipProps) {
  return (
    <span className="group relative inline-flex items-center">
      {children}
      <span
        role="tooltip"
        className={cn(
          "pointer-events-none absolute left-1/2 z-30 hidden -translate-x-1/2 whitespace-nowrap rounded-md border border-border bg-popover px-2 py-1 text-xs text-popover-foreground shadow-md group-hover:block group-focus-within:block",
          side === "top" ? "bottom-full mb-1" : "top-full mt-1",
          className,
        )}
      >
        {content}
      </span>
    </span>
  );
}
