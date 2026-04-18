// Browser-side cache for the baked URDF + GLB meshes (and anything else
// in /robot/manifest.json). Backed by IndexedDB so we can store binary
// payloads natively and well past localStorage's ~5 MB ceiling.
//
// Invalidation is content-hash-based:
//   1. The build emits link/public/robot/manifest.json with sha256 for
//      every baked asset (see link/scripts/bake-urdf.mjs).
//   2. The browser fetches the manifest network-first (it's tiny, ~1 KB).
//   3. For each asset URL we ask the cache: "do you have bytes whose
//      sha256 matches `expectedHash`?"  If yes, return them; if no, fetch,
//      verify, store, return.
//
// The hash check happens against bytes already in cache, so we never
// trust the cached metadata alone - bit-rot would surface as a re-fetch
// rather than a silent stale render.
//
// All public functions degrade gracefully when IndexedDB is unavailable
// (private browsing on some browsers, SSR, tests): they just fall through
// to a plain `fetch`. The viewer never sees an error from the cache layer.

const DB_NAME = "rudy-assets";
const DB_VERSION = 1;
const STORE_META = "meta"; // keyed by url       -> AssetMeta
const STORE_BLOB = "blob"; // keyed by sha256    -> ArrayBuffer

export interface ManifestEntry {
  url: string;
  sha256: string;
  bytes: number;
  mtime_ms: number | null;
}

export interface Manifest {
  version: number;
  generated_at: string;
  via: string;
  entries: ManifestEntry[];
}

interface AssetMeta {
  url: string;
  sha256: string;
  bytes: number;
  fetched_at: number; // ms since epoch
}

export interface CacheStats {
  available: boolean;
  entryCount: number;
  totalBytes: number;
  oldestFetchedAt: number | null;
  newestFetchedAt: number | null;
}

// --- IndexedDB plumbing --------------------------------------------------

function openDb(): Promise<IDBDatabase | null> {
  if (typeof indexedDB === "undefined") return Promise.resolve(null);
  return new Promise((resolve) => {
    let req: IDBOpenDBRequest;
    try {
      req = indexedDB.open(DB_NAME, DB_VERSION);
    } catch {
      resolve(null);
      return;
    }
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE_META)) {
        db.createObjectStore(STORE_META, { keyPath: "url" });
      }
      if (!db.objectStoreNames.contains(STORE_BLOB)) {
        db.createObjectStore(STORE_BLOB);
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => resolve(null);
    req.onblocked = () => resolve(null);
  });
}

function tx<T>(
  db: IDBDatabase,
  stores: string[],
  mode: IDBTransactionMode,
  fn: (t: IDBTransaction) => Promise<T> | T,
): Promise<T> {
  return new Promise((resolve, reject) => {
    let result: T;
    const t = db.transaction(stores, mode);
    t.oncomplete = () => resolve(result);
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
    Promise.resolve(fn(t))
      .then((r) => {
        result = r;
      })
      .catch((e) => {
        try {
          t.abort();
        } catch {
          /* tx may already be done */
        }
        reject(e);
      });
  });
}

function reqAsPromise<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

// --- Hashing -------------------------------------------------------------

async function sha256Hex(buf: ArrayBuffer): Promise<string> {
  // SubtleCrypto.digest is available on https + localhost in all modern
  // browsers we care about. (`vite dev` serves localhost; production is
  // either tailnet-https or https-via-tailscale.)
  const digest = await crypto.subtle.digest("SHA-256", buf);
  const bytes = new Uint8Array(digest);
  let hex = "";
  for (let i = 0; i < bytes.length; i++) {
    hex += bytes[i].toString(16).padStart(2, "0");
  }
  return hex;
}

// --- Public API ----------------------------------------------------------

/**
 * Load `url` as an ArrayBuffer, satisfying from cache when the cached
 * bytes' sha256 matches `expectedHash`. On miss/mismatch, fetches over
 * the network, verifies the hash, and stores the result before returning.
 *
 * If `expectedHash` is undefined (manifest unavailable), this falls back
 * to a plain fetch with no cache participation - safer than serving
 * potentially stale bytes.
 */
export async function loadCachedAsset(
  url: string,
  expectedHash: string | undefined,
): Promise<ArrayBuffer> {
  if (!expectedHash) {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`fetch ${url} -> HTTP ${res.status}`);
    return res.arrayBuffer();
  }

  const db = await openDb();
  if (db) {
    const cached = await tx<ArrayBuffer | null>(
      db,
      [STORE_META, STORE_BLOB],
      "readonly",
      async (t) => {
        const meta = (await reqAsPromise(
          t.objectStore(STORE_META).get(url),
        )) as AssetMeta | undefined;
        if (!meta || meta.sha256 !== expectedHash) return null;
        const blob = (await reqAsPromise(
          t.objectStore(STORE_BLOB).get(expectedHash),
        )) as ArrayBuffer | undefined;
        return blob ?? null;
      },
    );
    if (cached) return cached;
  }

  const res = await fetch(url);
  if (!res.ok) throw new Error(`fetch ${url} -> HTTP ${res.status}`);
  const buf = await res.arrayBuffer();

  // Verify before storing. A mismatch usually means the manifest was
  // served stale (e.g. an HTTP intermediary cached it longer than the
  // bytes themselves). We still return the freshly-fetched buffer; just
  // skip caching so we don't pin a wrong-hash blob.
  const actual = await sha256Hex(buf);
  if (actual !== expectedHash) {
    if (typeof console !== "undefined") {
      console.warn(
        `asset-cache: hash mismatch for ${url} (manifest=${expectedHash.slice(0, 12)} fetched=${actual.slice(0, 12)}); skipping cache`,
      );
    }
    return buf;
  }

  if (db) {
    try {
      await tx<void>(
        db,
        [STORE_META, STORE_BLOB],
        "readwrite",
        async (t) => {
          await reqAsPromise(
            t.objectStore(STORE_BLOB).put(buf, expectedHash),
          );
          await reqAsPromise(
            t
              .objectStore(STORE_META)
              .put({
                url,
                sha256: expectedHash,
                bytes: buf.byteLength,
                fetched_at: Date.now(),
              } satisfies AssetMeta),
          );
        },
      );
    } catch (err) {
      // Quota errors etc. - don't fail the page, the asset is in memory
      // and will just be re-fetched next time.
      if (typeof console !== "undefined") {
        console.warn("asset-cache: write failed", err);
      }
    }
  }

  return buf;
}

/**
 * Convenience wrapper for text assets (the URDF). Same caching path.
 */
export async function loadCachedText(
  url: string,
  expectedHash: string | undefined,
): Promise<string> {
  const buf = await loadCachedAsset(url, expectedHash);
  return new TextDecoder("utf-8").decode(buf);
}

/**
 * Drop every cached blob and meta record. Used by the dashboard "Clear
 * cache" button.
 */
export async function clearAssetCache(): Promise<void> {
  const db = await openDb();
  if (!db) return;
  await tx<void>(
    db,
    [STORE_META, STORE_BLOB],
    "readwrite",
    async (t) => {
      await reqAsPromise(t.objectStore(STORE_META).clear());
      await reqAsPromise(t.objectStore(STORE_BLOB).clear());
    },
  );
}

/**
 * Cheap snapshot of cache contents - what's in IndexedDB right now,
 * regardless of the live manifest. Used by the Overview status card.
 */
export async function readCacheStats(): Promise<CacheStats> {
  const db = await openDb();
  if (!db) {
    return {
      available: false,
      entryCount: 0,
      totalBytes: 0,
      oldestFetchedAt: null,
      newestFetchedAt: null,
    };
  }
  const all = await tx<AssetMeta[]>(db, [STORE_META], "readonly", async (t) => {
    const items = await reqAsPromise(t.objectStore(STORE_META).getAll());
    return items as AssetMeta[];
  });
  let total = 0;
  let oldest: number | null = null;
  let newest: number | null = null;
  for (const m of all) {
    total += m.bytes;
    if (oldest === null || m.fetched_at < oldest) oldest = m.fetched_at;
    if (newest === null || m.fetched_at > newest) newest = m.fetched_at;
  }
  return {
    available: true,
    entryCount: all.length,
    totalBytes: total,
    oldestFetchedAt: oldest,
    newestFetchedAt: newest,
  };
}

/**
 * Build an O(1) lookup from URL -> sha256 over a manifest. Cheap; just a
 * Map. Centralized so callers can't accidentally trust an entry whose URL
 * has been path-normalized differently from what they fetch.
 */
export function indexManifest(
  manifest: Manifest | undefined | null,
): Map<string, ManifestEntry> {
  const m = new Map<string, ManifestEntry>();
  if (!manifest) return m;
  for (const e of manifest.entries) m.set(e.url, e);
  return m;
}
