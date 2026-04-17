import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { TelemetryGrid } from "@/components/telemetry-grid";

export const Route = createFileRoute("/_authed/telemetry")({
  component: TelemetryPage,
});

function TelemetryPage() {
  const motorsQ = useQuery({
    queryKey: ["motors"],
    queryFn: () => api.listMotors(),
    refetchInterval: 1_000, // Phase-1 fallback while WebTransport is offline.
  });
  const configQ = useQuery({ queryKey: ["config"], queryFn: () => api.config() });

  if (motorsQ.isPending || configQ.isPending) {
    return <div className="text-muted-foreground">Loading...</div>;
  }
  if (motorsQ.isError) {
    return <div className="text-destructive">Error: {(motorsQ.error as Error).message}</div>;
  }

  return (
    <div className="space-y-4">
      <header className="flex items-baseline justify-between">
        <h1 className="text-2xl font-semibold">Telemetry</h1>
        <div className="text-xs text-muted-foreground">
          {configQ.data?.features.mock_can ? "mock CAN (no hardware)" : "live CAN"} -{" "}
          actuator: {configQ.data?.actuator_model}
        </div>
      </header>
      <TelemetryGrid motors={motorsQ.data ?? []} />
    </div>
  );
}
