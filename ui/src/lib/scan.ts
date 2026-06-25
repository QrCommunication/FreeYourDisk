import { writable, derived, get } from "svelte/store";
import {
  api,
  SERVICES,
  type MountUsage,
  type ScanItem,
  type ServiceId,
  type TypeBucket,
} from "./api";
import { disjointItems } from "./reclaim";

export interface UnifiedItem extends ScanItem {
  service: ServiceId;
  /** New or changed since the previous scan of its service. */
  isNew: boolean;
}

export type ScanStatus = "idle" | "scanning" | "done";

export const scanStatus = writable<ScanStatus>("idle");
export const scanProgress = writable<{ done: number; total: number }>({
  done: 0,
  total: SERVICES.length,
});
export const items = writable<UnifiedItem[]>([]);
export const selection = writable<Set<string>>(new Set());
export const primaryUsage = writable<MountUsage | null>(null);
export const fileTypes = writable<TypeBucket[]>([]);
/** Total bytes under the home dir — used to size the real system footprint. */
export const homeTotal = writable<number>(0);
/** Measured OS footprint (readable non-home dirs + swap). */
export const systemTotal = writable<number>(0);
/** True while a background refresh runs over already-displayed results. */
export const refreshing = writable(false);
/** Unix ms of the last completed scan (null if never). */
export const lastScanAt = writable<number | null>(null);

// Mutually-exclusive items: each path counted once, assigned to a single
// category. Per-group totals derived from this SUM to `reclaimableBytes`.
export const disjoint = derived(items, ($items) => disjointItems($items));

export const reclaimableBytes = derived(disjoint, ($disjoint) =>
  $disjoint.reduce((sum, item) => sum + item.size_bytes, 0),
);
export const selectedBytes = derived(
  [disjoint, selection],
  ([$disjoint, $selection]) =>
    $disjoint
      .filter((item) => $selection.has(item.id))
      .reduce((sum, item) => sum + item.size_bytes, 0),
);
export const selectedCount = derived(
  selection,
  ($selection) => $selection.size,
);

/** Load the primary mount (root, or the largest) for the donut base. */
export async function loadUsage(): Promise<void> {
  try {
    const mounts = await api.diskUsage();
    const root =
      mounts.find((m) => m.mount === "/") ??
      mounts.slice().sort((a, b) => b.total - a.total)[0] ??
      null;
    primaryUsage.set(root);
  } catch {
    primaryUsage.set(null);
  }
}

/**
 * Run all four scans + the file-type breakdown concurrently.
 * `background`: keep the currently-displayed (cached) results visible and only
 * swap them in when the refresh completes (no "scanning" flash).
 */
export async function runAllScans(background = false): Promise<void> {
  if (background) {
    refreshing.set(true);
  } else {
    scanStatus.set("scanning");
    items.set([]);
    selection.set(new Set());
    fileTypes.set([]);
  }
  scanProgress.set({ done: 0, total: SERVICES.length });
  await loadUsage();

  // File-type breakdown + home total walk the home too — run them alongside.
  const typesPromise = api.fileTypes().catch(() => [] as TypeBucket[]);
  const homePromise = api.homeTotal().catch(() => 0);
  const systemPromise = api.systemTotal().catch(() => 0);

  const collected: UnifiedItem[] = [];
  await Promise.all(
    SERVICES.map(async (service) => {
      try {
        const resp = await api.scan(service);
        const fresh = new Set(resp.new_ids);
        const mapped: UnifiedItem[] = resp.result.items.map((item) => ({
          ...item,
          service,
          isNew: !resp.first_scan && fresh.has(item.id),
        }));
        collected.push(...mapped);
        if (!background) items.update((current) => [...current, ...mapped]);
      } catch {
        /* a failing service is skipped, others still populate */
      }
      scanProgress.update((p) => ({ ...p, done: p.done + 1 }));
    }),
  );
  const types = await typesPromise;

  items.set(collected);
  fileTypes.set(types);
  homeTotal.set(await homePromise);
  systemTotal.set(await systemPromise);
  scanStatus.set("done");
  refreshing.set(false);
  lastScanAt.set(Date.now());
  void persistHomeCache();
}

/** Persist the latest results for instant display next launch. */
async function persistHomeCache(): Promise<void> {
  try {
    const data = JSON.stringify({
      items: get(items),
      fileTypes: get(fileTypes),
      homeTotal: get(homeTotal),
      systemTotal: get(systemTotal),
      usage: get(primaryUsage),
      ts: get(lastScanAt),
    });
    await api.homeCacheSave(data);
  } catch {
    /* non-fatal */
  }
}

/** Restore cached results on open (instant). Returns true if data was shown. */
export async function restoreHomeCache(): Promise<boolean> {
  try {
    const raw = await api.homeCacheLoad();
    if (!raw) return false;
    const data = JSON.parse(raw);
    if (!Array.isArray(data.items) || data.items.length === 0) return false;
    items.set(data.items);
    fileTypes.set(data.fileTypes ?? []);
    homeTotal.set(data.homeTotal ?? 0);
    systemTotal.set(data.systemTotal ?? 0);
    if (data.usage) primaryUsage.set(data.usage);
    lastScanAt.set(data.ts ?? null);
    scanStatus.set("done");
    return true;
  } catch {
    return false;
  }
}

export function toggle(id: string): void {
  selection.update((set) => {
    const next = new Set(set);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  });
}

export function setMany(ids: string[], on: boolean): void {
  selection.update((set) => {
    const next = new Set(set);
    for (const id of ids) {
      if (on) next.add(id);
      else next.delete(id);
    }
    return next;
  });
}

export function clearScan(): void {
  scanStatus.set("idle");
  items.set([]);
  selection.set(new Set());
}
