// ECharts and three.js cannot read CSS custom properties directly, so we read
// the concrete `--c-*` runtime tokens (literal hex, swapped by the theme).

/** Read a CSS custom property's resolved value, with a fallback. */
export function cssColor(token: string, fallback = "#000000"): string {
  if (typeof document === "undefined") return fallback;
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue(token)
    .trim();
  return value || fallback;
}

/** Current theme palette for charts/3D (re-read after a theme switch). */
export function chartColors() {
  return {
    accent: cssColor("--c-accent", "#2dd4bf"),
    savings: cssColor("--c-savings", "#fbbf24"),
    freed: cssColor("--c-freed", "#34d399"),
    ink: cssColor("--c-ink", "#e6edf3"),
    muted: cssColor("--c-muted", "#8b97a5"),
    faint: cssColor("--c-faint", "#5b6573"),
    line: cssColor("--c-line", "#1f2630"),
    surface: cssColor("--c-surface", "#161c25"),
    danger: cssColor("--c-danger", "#fb7185"),
    track: cssColor("--c-line", "#1f2630"),
  };
}

// Live getters so existing chart components pick up theme changes on re-render.
export const C = {
  get accent() {
    return cssColor("--c-accent", "#2dd4bf");
  },
  get accentDim() {
    return cssColor("--c-accent-soft", "#0f3d39");
  },
  get ink() {
    return cssColor("--c-ink", "#e6edf3");
  },
  get muted() {
    return cssColor("--c-muted", "#8b97a5");
  },
  get faint() {
    return cssColor("--c-faint", "#5b6573");
  },
  get danger() {
    return cssColor("--c-danger", "#fb7185");
  },
  get surface() {
    return cssColor("--c-surface", "#161c25");
  },
  get line() {
    return cssColor("--c-line", "#1f2630");
  },
  get base() {
    return cssColor("--c-base", "#0b0e14");
  },
} as const;

// Sequential palette for treemaps / bars.
export const SERIES = [
  "#2dd4bf",
  "#38bdf8",
  "#22c55e",
  "#fbbf24",
  "#fb923c",
  "#f472b6",
  "#a78bfa",
];
