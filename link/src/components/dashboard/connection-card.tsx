// Small "what am I talking to?" card. Shows version, build stamp, GitHub
// staleness, actuator model, CAN mode (live/mock), and WebTransport advert.

import { useConfigQuery } from "@/api";
import { cn } from "@/lib/utils";
import { DashboardCard } from "./dashboard-card";

function formatWhen(s: string | null | undefined) {
  if (s == null || s === "") return "—";
  const t = Date.parse(s);
  if (Number.isNaN(t)) return s;
  return new Date(t).toLocaleString();
}

const CONFIG_POLL_DEPLOYMENT_MS = 30_000;

export function ConnectionCard({ className }: { className?: string }) {
  // Overview-only card: re-fetch /api/config so `deployment` (stale vs
  // `latest.json`, updater) tracks the server; interval stops on navigate away.
  const q = useConfigQuery({ refetchInterval: CONFIG_POLL_DEPLOYMENT_MS });
  const cfg = q.data;
  const d = cfg?.deployment;

  return (
    <DashboardCard
      title="Connection"
      className={className}
      hint={
        d ? (
          <span title={`${d.build.commit_sha} @ ${d.build.built_at}`}>
            v{d.build.package_version} · {d.build.short_sha}
          </span>
        ) : cfg ? (
          <span>v{cfg.version}</span>
        ) : undefined
      }
    >
      {q.isPending && (
        <div className="text-sm text-muted-foreground">loading...</div>
      )}
      {q.isError && (
        <div className="text-sm text-destructive">
          {(q.error as Error).message}
        </div>
      )}
      {cfg && (
        <dl className="grid grid-cols-2 gap-2 text-xs">
          {d && (
            <Field className="col-span-2" label="Releases (GitHub)">
              {d.latest.manifest_error != null && d.latest.manifest_error !== "" ? (
                <span
                  className="text-amber-400"
                  title={d.latest.manifest_error}
                >
                  can&apos;t check ({d.latest.manifest_error.length > 48 ? `${d.latest.manifest_error.slice(0, 45)}…` : d.latest.manifest_error})
                </span>
              ) : d.is_stale ? (
                <span className="text-amber-400">
                  stale — latest{" "}
                  <span className="font-mono">
                    {d.latest.short_sha ?? d.latest.commit_sha?.slice(0, 12) ?? "—"}
                  </span>
                </span>
              ) : d.latest_manifest_ok ? (
                <span className="text-emerald-400">up to date</span>
              ) : (
                <span className="text-muted-foreground">unknown</span>
              )}
            </Field>
          )}
          {d && (
            <>
              <Field label="Last update check (timer)">
                {d.updater.last_check?.trim()
                  ? d.updater.last_check
                  : "—"}
              </Field>
              <Field label="Last applied (current.sha)">
                {formatWhen(d.updater.last_applied ?? null)}
              </Field>
              <Field className="col-span-2" label="Updater health">
                {d.updater.systemd_probed ? (
                  <span
                    className={cn(
                      d.updater.healthy
                        ? "text-emerald-400"
                        : "text-destructive",
                    )}
                  >
                    {d.updater.healthy ? "ok" : "unhealthy"}
                    {d.updater.timer_active != null && (
                      <span className="ml-1.5 text-muted-foreground">
                        · timer {d.updater.timer_active ? "on" : "off"}
                        {d.updater.service_failed != null && (
                          <>
                            {d.updater.service_failed
                              ? " · service failed"
                              : " · service ok"}
                          </>
                        )}
                      </span>
                    )}
                  </span>
                ) : (
                  <span className="text-muted-foreground">n/a (this host)</span>
                )}
              </Field>
            </>
          )}
          <Field label="Actuator">{cfg.actuator_models.join(", ")}</Field>
          <Field label="CAN">
            <span
              className={cn(
                "rounded-sm px-1.5 py-0.5",
                cfg.features.mock_can
                  ? "bg-amber-500/10 text-amber-400"
                  : "bg-emerald-500/10 text-emerald-400",
              )}
            >
              {cfg.features.mock_can ? "mock" : "live"}
            </span>
          </Field>
          <Field label="WebTransport">
            {cfg.webtransport.enabled ? (
              <span
                className="block truncate font-mono text-[11px]"
                title={cfg.webtransport.url ?? undefined}
              >
                {cfg.webtransport.url ?? "advertised"}
              </span>
            ) : (
              <span className="text-muted-foreground">disabled</span>
            )}
          </Field>
          <Field label="Verification">
            <span
              className={cn(
                cfg.features.require_verified
                  ? "text-emerald-400"
                  : "text-muted-foreground",
              )}
            >
              {cfg.features.require_verified ? "required" : "optional"}
            </span>
          </Field>
        </dl>
      )}
    </DashboardCard>
  );
}

function Field({
  label,
  children,
  className,
}: {
  label: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "rounded-md border border-border/60 bg-background px-2 py-1.5",
        className,
      )}
    >
      <div className="text-muted-foreground">{label}</div>
      <div className="mt-0.5">{children}</div>
    </div>
  );
}
