// Small "what am I talking to?" card. Shows version, actuator model,
// CAN mode (live/mock), and WebTransport advert. Mostly diagnostic.

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { cn } from "@/lib/utils";
import { DashboardCard } from "./dashboard-card";

export function ConnectionCard({ className }: { className?: string }) {
  const q = useQuery({ queryKey: ["config"], queryFn: () => api.config() });
  const cfg = q.data;

  return (
    <DashboardCard
      title="Connection"
      className={className}
      hint={cfg ? <span>v{cfg.version}</span> : undefined}
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
          <Field label="Actuator">{cfg.actuator_model}</Field>
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
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="rounded-md border border-border/60 bg-background px-2 py-1.5">
      <div className="text-muted-foreground">{label}</div>
      <div className="mt-0.5">{children}</div>
    </div>
  );
}
