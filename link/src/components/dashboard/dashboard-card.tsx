// Shared frame for dashboard widgets. Title + optional right-hand
// adornment + body. The grid in `dashboard-grid.tsx` arranges N of these.

import { cn } from "@/lib/utils";

export interface DashboardCardProps {
  title: string;
  hint?: React.ReactNode;
  className?: string;
  bodyClassName?: string;
  children: React.ReactNode;
}

export function DashboardCard({
  title,
  hint,
  className,
  bodyClassName,
  children,
}: DashboardCardProps) {
  return (
    <section
      className={cn(
        "flex flex-col rounded-lg border border-border bg-card p-4",
        className,
      )}
    >
      <header className="mb-3 flex items-baseline justify-between gap-2">
        <h2 className="text-sm font-semibold tracking-tight">{title}</h2>
        {hint != null && (
          <span className="text-xs text-muted-foreground">{hint}</span>
        )}
      </header>
      <div className={cn("flex-1", bodyClassName)}>{children}</div>
    </section>
  );
}
