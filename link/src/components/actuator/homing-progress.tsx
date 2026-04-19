/** Shared progress bar for `BootState` auto-homing (orchestrator or legacy shape). */

export function HomingProgressBar({
  fromRad,
  targetRad,
  progressRad,
}: {
  fromRad: number;
  targetRad: number;
  progressRad: number;
}) {
  const span =
    Math.abs(targetRad - fromRad) > 1e-6
      ? Math.min(
          1,
          Math.max(0, (progressRad - fromRad) / (targetRad - fromRad)),
        )
      : 1;

  return (
    <div className="h-1.5 w-full max-w-[11rem] overflow-hidden rounded-full bg-muted">
      <div
        className="h-full bg-sky-500 transition-[width] duration-300"
        style={{ width: `${span * 100}%` }}
      />
    </div>
  );
}
