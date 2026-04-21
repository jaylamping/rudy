// React-flavored façade over `../asset-cache.ts`. Keeps TanStack Query as
// the single source of "manifest" truth and exposes a refreshable cache
// stats query.

import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseQueryResult,
} from "@tanstack/react-query";
import { queryKeys } from "@/api";
import {
  clearAssetCache,
  indexManifest,
  readCacheStats,
  type CacheStats,
  type Manifest,
} from "../asset-cache";

const MANIFEST_URL = "/robot/manifest.json";

async function fetchManifest(): Promise<Manifest> {
  // Manifest itself is intentionally NOT cached in IndexedDB. It's tiny
  // (~1 KB) and lives one HTTP round-trip away; we want to detect new
  // bakes immediately. `cache: "no-store"` defeats the SW/HTTP cache so
  // a redeploy is visible without a hard reload.
  const res = await fetch(MANIFEST_URL, { cache: "no-store" });
  if (!res.ok) {
    throw new Error(`fetch ${MANIFEST_URL} -> HTTP ${res.status}`);
  }
  return (await res.json()) as Manifest;
}

export function useAssetManifest(): UseQueryResult<Manifest> {
  return useQuery({
    queryKey: queryKeys.assets.manifest(),
    queryFn: fetchManifest,
    staleTime: 30_000,
    refetchOnWindowFocus: true,
    retry: 1,
  });
}

/**
 * Read-only snapshot of what's currently in IndexedDB. Refreshes on
 * mount and whenever queries are invalidated (e.g. after a clear).
 */
export function useCacheStats(): UseQueryResult<CacheStats> {
  return useQuery({
    queryKey: queryKeys.assets.cacheStats(),
    queryFn: () => readCacheStats(),
    staleTime: 5_000,
  });
}

/**
 * Mutation: drop every cached blob, then invalidate the stats query so
 * the UI snaps to "0 entries".
 */
export function useClearAssetCache() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => clearAssetCache(),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: queryKeys.assets.cacheStats() });
    },
  });
}

export { indexManifest };
export type { CacheStats, Manifest };
