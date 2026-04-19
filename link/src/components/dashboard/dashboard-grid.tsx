// CSS grid for the Overview page. Adding a new card is one line in
// `_app.index.tsx` — children just drop in.

import { cn } from "@/lib/utils";

export function DashboardGrid({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        // 12-col grid on lg+, single column on small. Each card carries
        // its own col-span via its className.
        "grid grid-cols-1 gap-4 lg:grid-cols-12",
        className,
      )}
    >
      {children}
    </div>
  );
}
