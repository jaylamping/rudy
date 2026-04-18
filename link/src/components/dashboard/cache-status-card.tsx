// Show what the browser has cached out of /robot/manifest.json (URDF +
// meshes) and how that compares to the latest bake. "Clear" wipes the
// IndexedDB store; the next viz load will refetch.

import { Trash2 } from "lucide-react";
import { useMemo } from "react";
import {
  useAssetManifest,
  useCacheStats,
  useClearAssetCache,
} from "@/lib/use-asset-cache";
import { cn } from "@/lib/utils";
import { DashboardCard } from "./dashboard-card";

export function CacheStatusCard({ className }: { className?: string }) {
  const stats = useCacheStats();
  const manifest = useAssetManifest();
  const clear = useClearAssetCache();

  const manifestBytes = useMemo(() => {
    if (!manifest.data) return null;
    return manifest.data.entries.reduce((acc, e) => acc + e.bytes, 0);
  }, [manifest.data]);

  const lastBakedMs = useMemo(() => {
    if (!manifest.data) return null;
    const t = Date.parse(manifest.data.generated_at);
    return Number.isFinite(t) ? t : null;
  }, [manifest.data]);

  const cacheAvailable = stats.data?.available ?? false;
  const fullyCached =
    cacheAvailable &&
    manifest.data != null &&
    stats.data!.entryCount >= manifest.data.entries.length;

  return (
    <DashboardCard
      title="Asset cache"
      className={className}
      hint={
        manifest.data ? (
          <span title={manifest.data.generated_at}>
            baked {fmtRelativePast(lastBakedMs)}
          </span>
        ) : undefined
      }
    >
      <div className="grid grid-cols-2 gap-2 text-xs">
        <Field
          label="Cached"
          value={
            cacheAvailable
              ? `${stats.data!.entryCount} / ${manifest.data?.entries.length ?? "?"}`
              : "n/a"
          }
          tone={
            !cacheAvailable
              ? "muted"
              : fullyCached
                ? "ok"
                : stats.data!.entryCount === 0
                  ? "muted"
                  : "warn"
          }
        />
        <Field
          label="On disk"
          value={
            cacheAvailable ? fmtBytes(stats.data!.totalBytes) : "n/a"
          }
        />
        <Field
          label="Bake total"
          value={manifestBytes != null ? fmtBytes(manifestBytes) : "?"}
        />
        <Field
          label="Newest fetch"
          value={fmtRelativePast(stats.data?.newestFetchedAt ?? null)}
        />
      </div>

      {!cacheAvailable && (
        <p className="mt-3 text-xs text-muted-foreground">
          IndexedDB unavailable in this context (private browsing?). Assets
          will be fetched from the network on every load.
        </p>
      )}

      <div className="mt-3 flex items-center gap-2">
        <button
          type="button"
          onClick={() => clear.mutate()}
          disabled={
            !cacheAvailable ||
            clear.isPending ||
            (stats.data?.entryCount ?? 0) === 0
          }
          className={cn(
            "flex items-center gap-1 rounded-md border border-border bg-background px-2 py-1 text-xs transition",
            "hover:border-rose-500/40 hover:text-rose-400",
            (clear.isPending ||
              !cacheAvailable ||
              (stats.data?.entryCount ?? 0) === 0) &&
              "cursor-not-allowed opacity-50",
          )}
        >
          <Trash2 className="h-3.5 w-3.5" />
          {clear.isPending ? "Clearing..." : "Clear cache"}
        </button>
        {clear.isError && (
          <span className="text-xs text-destructive">
            {(clear.error as Error).message}
          </span>
        )}
      </div>
    </DashboardCard>
  );
}

function Field({
  label,
  value,
  tone = "muted",
}: {
  label: string;
  value: string;
  tone?: "ok" | "warn" | "muted";
}) {
  return (
    <div className="rounded-md border border-border/60 bg-background px-2 py-1.5">
      <div className="text-muted-foreground">{label}</div>
      <div
        className={cn(
          "mt-0.5 font-mono tabular-nums",
          tone === "ok" && "text-emerald-400",
          tone === "warn" && "text-amber-400",
        )}
      >
        {value}
      </div>
    </div>
  );
}

function fmtBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "?";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}

function fmtRelativePast(t: number | null): string {
  if (t == null) return "--";
  const ms = Date.now() - t;
  if (ms < 0) return "just now";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}h ago`;
  const d = Math.round(h / 24);
  return `${d}d ago`;
}
