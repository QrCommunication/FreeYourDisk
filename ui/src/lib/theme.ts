// Concrete hex values mirroring the CSS @theme tokens, for ECharts (which
// cannot read CSS custom properties).
export const C = {
  accent: "#2dd4bf",
  accentDim: "#0f3d39",
  ink: "#e6edf3",
  muted: "#8b97a5",
  faint: "#5b6573",
  danger: "#fb7185",
  surface: "#161c25",
  line: "#1f2630",
  base: "#0b0e14",
} as const;

// Sequential palette for treemaps / bars (single-hue, desaturated steps).
export const SERIES = [
  "#2dd4bf",
  "#38bdf8",
  "#818cf8",
  "#a78bfa",
  "#f472b6",
  "#fb7185",
  "#fbbf24",
];
