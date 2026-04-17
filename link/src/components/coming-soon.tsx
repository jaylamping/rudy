export function ComingSoon({
  title,
  phase,
  points,
}: {
  title: string;
  phase: string;
  points: string[];
}) {
  return (
    <div className="space-y-4">
      <header className="flex items-baseline justify-between">
        <h1 className="text-2xl font-semibold">{title}</h1>
        <span className="rounded-sm bg-muted px-2 py-0.5 text-xs text-muted-foreground">
          {phase}
        </span>
      </header>
      <div className="rounded-lg border border-dashed border-border bg-card p-6">
        <p className="text-sm text-muted-foreground">
          This surface is planned but not yet implemented. Phase 1 lands the
          parameter editor and telemetry; the remaining surfaces follow in
          Phase 2.
        </p>
        <ul className="mt-4 list-disc space-y-1 pl-6 text-sm">
          {points.map((p) => (
            <li key={p}>{p}</li>
          ))}
        </ul>
      </div>
    </div>
  );
}
