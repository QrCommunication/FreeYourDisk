import type { ServiceId } from "./api";

interface PathSized {
  path: string;
  size_bytes: number;
}

/**
 * Sum item sizes while removing double-counting: identical paths are counted
 * once, and a path nested inside another kept path is dropped (its bytes are
 * already in the ancestor). This prevents totals from exceeding the disk size
 * when services overlap or return nested directories.
 */
export function dedupSize(items: PathSized[]): number {
  // Ancestors are always shorter than their descendants, so length-sort puts
  // every ancestor before its children.
  const sorted = [...items].sort((a, b) => a.path.length - b.path.length);
  const kept: string[] = [];
  const seen = new Set<string>();
  let total = 0;
  for (const item of sorted) {
    if (seen.has(item.path)) continue;
    seen.add(item.path);
    const prefixed = kept.some(
      (k) =>
        item.path === k || item.path.startsWith(k.endsWith("/") ? k : k + "/"),
    );
    if (!prefixed) {
      kept.push(item.path);
      total += item.size_bytes;
    }
  }
  return total;
}

// A path is assigned to the highest-priority category that claims it. Git
// worktrees outrank dev caches and big files: a whole worktree (which may
// contain its own node_modules) stays in the Git section instead of being
// split into dev-cache / big-files.
const PRIORITY: Record<ServiceId, number> = {
  temp: 0,
  app_cache: 1,
  git_repos: 2,
  dev_cache: 3,
  big_files: 4,
};

function related(a: string, b: string): boolean {
  if (a === b) return true;
  const ad = a.endsWith("/") ? a : a + "/";
  const bd = b.endsWith("/") ? b : b + "/";
  return a.startsWith(bd) || b.startsWith(ad); // ancestor or descendant
}

/**
 * Return a mutually-exclusive subset of items: no two kept paths overlap (none
 * is an ancestor/descendant of another; no duplicates). Each path resolves to a
 * single category by priority, so per-category totals SUM to the real
 * reclaimable footprint and never exceed the disk.
 */
export function disjointItems<T extends PathSized & { service: ServiceId }>(
  items: T[],
): T[] {
  const sorted = [...items].sort((a, b) => {
    const p = PRIORITY[a.service] - PRIORITY[b.service];
    if (p !== 0) return p; // higher-priority category wins overlaps
    return a.path.length - b.path.length; // ancestors before descendants
  });
  const kept: T[] = [];
  const seen = new Set<string>();
  for (const item of sorted) {
    if (seen.has(item.path)) continue;
    seen.add(item.path);
    if (!kept.some((k) => related(item.path, k.path))) kept.push(item);
  }
  return kept;
}
