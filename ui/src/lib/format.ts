const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** Human-readable byte size, e.g. 1536 -> "1.5 KB". */
export function humanizeBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 1) return "0 B";
  const i = Math.min(
    UNITS.length - 1,
    Math.floor(Math.log(bytes) / Math.log(1024)),
  );
  const value = bytes / 1024 ** i;
  const decimals = i === 0 || value >= 100 ? 0 : 1;
  return `${value.toFixed(decimals)} ${UNITS[i]}`;
}

/** Localised short date from a Unix timestamp (seconds), or null. */
export function humanizeDate(unixSecs: number | null): string | null {
  if (unixSecs == null) return null;
  const d = new Date(unixSecs * 1000);
  if (Number.isNaN(d.getTime())) return null;
  return d.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/** Percentage (0-100, one decimal) of used/total, clamped. */
export function usedPercent(used: number, total: number): number {
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, (used / total) * 100));
}

/** Compact uptime from seconds, e.g. 93600 -> "1d 2h". */
export function humanizeUptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

/** Throughput in B/s -> "12.4 MB/s". */
export function humanizeRate(bytesPerSec: number): string {
  return `${humanizeBytes(Math.max(0, bytesPerSec))}/s`;
}
