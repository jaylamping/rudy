import type { ReactNode } from "react";

/**
 * Section wrapper for the Devices page so groupings (limb cards, unassigned
 * scan, sensors, batteries) stay visually consistent.
 */
export function DevicesSection({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <section className="mb-10">
      <div className="mb-3">
        <h2 className="text-lg font-semibold tracking-tight">{title}</h2>
        {description ? (
          <p className="mt-1 text-sm text-muted-foreground">{description}</p>
        ) : null}
      </div>
      {children}
    </section>
  );
}
