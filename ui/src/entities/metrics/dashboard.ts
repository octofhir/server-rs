import type { InstantSample, Series } from "./api";

/**
 * Dashboard documents, the same shape the server validates and exports to Grafana
 * (`crates/octofhir-server/src/prometheus/dashboards.rs`): a 24-column grid, Grafana unit ids and
 * `$variables`, so nothing is lost on export.
 */

export type PanelType = "timeseries" | "stat" | "table" | "row";
export type DrawStyle = "line" | "bars";
export type SeriesColor = "neutral" | "accent" | "success" | "warning" | "error" | "info";

export interface GridPos {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface PanelTarget {
  refId: string;
  expr: string;
  /** Series name template, e.g. `{{status_class}}`; the column title in tables. */
  legend?: string;
  /** Overrides the panel's unit for this query (table columns of different kinds). */
  unit?: string;
}

export interface Threshold {
  value: number;
  color: SeriesColor;
}

export interface Panel {
  id: string;
  title: string;
  type: PanelType;
  description?: string;
  gridPos: GridPos;
  targets: PanelTarget[];
  unit?: string;
  decimals?: number;
  min?: number;
  max?: number;
  drawStyle?: DrawStyle;
  stack?: boolean;
  colors?: Record<string, SeriesColor>;
  thresholds?: Threshold[];
}

export interface DashboardVariable {
  name: string;
  label?: string;
  labelName: string;
  metric?: string;
  multi?: boolean;
  includeAll?: boolean;
}

/** How hovering one chart shows on the others (Grafana's `graphTooltip`). */
export type CursorSync = "off" | "crosshair" | "tooltip";

export const CURSOR_SYNC_OPTIONS: { value: CursorSync; label: string }[] = [
  { value: "crosshair", label: "Shared crosshair" },
  { value: "tooltip", label: "Shared tooltip" },
  { value: "off", label: "Independent" },
];

export interface Dashboard {
  /** Built-in id, or the `Dashboard` resource id. */
  id?: string;
  title: string;
  description?: string;
  time?: { from: string; to: string };
  variables?: DashboardVariable[];
  cursorSync?: CursorSync;
  panels: Panel[];
}

export const GRID_COLUMNS = 24;

export const ALL_VALUE = "$__all";

/** Time ranges the toolbar offers, in Grafana notation. */
export const TIME_RANGES = [
  { value: "now-15m", label: "15m" },
  { value: "now-1h", label: "1h" },
  { value: "now-6h", label: "6h" },
  { value: "now-24h", label: "24h" },
  { value: "now-7d", label: "7d" },
] as const;

const UNIT_SECONDS: Record<string, number> = { s: 1, m: 60, h: 3600, d: 86_400, w: 604_800 };

/** `now-1h` → 3600; anything else falls back to an hour. */
export function rangeSeconds(from: string): number {
  const match = /^now-(\d+)([smhdw])$/.exec(from.trim());
  if (!match) return 3600;
  return Number(match[1]) * UNIT_SECONDS[match[2]];
}

/** Resolved time window: relative ones follow the clock, absolute ones (a zoom) stay put. */
export interface TimeWindow {
  /** Seconds the window spans. */
  range: number;
  /** Absolute end in unix seconds, or `null` when the window ends now. */
  end: number | null;
}

/**
 * `from`/`to` as kept in the URL: Grafana's relative `now-1h`/`now`, or absolute epoch
 * milliseconds after a drag-to-zoom.
 */
export function resolveWindow(from: string, to: string | null): TimeWindow {
  const start = Number(from);
  const end = Number(to);
  if (to && Number.isFinite(start) && Number.isFinite(end) && end > start) {
    return { range: Math.max(1, Math.round((end - start) / 1000)), end: Math.round(end / 1000) };
  }
  return { range: rangeSeconds(from), end: null };
}

/** Short label of an absolute window, e.g. `14:02 – 14:20`. */
export function windowLabel(startMs: number, endMs: number): string {
  const sameDay = new Date(startMs).toDateString() === new Date(endMs).toDateString();
  const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  const day = (ms: number) => new Date(ms).toLocaleDateString([], { month: "short", day: "numeric" });
  return sameDay ? `${time(startMs)} – ${time(endMs)}` : `${day(startMs)} ${time(startMs)} – ${day(endMs)} ${time(endMs)}`;
}

/** `{{label}}` templates filled from a series' labels; without one, the labels themselves. */
export function seriesName(legend: string | undefined, labels: Record<string, string>): string {
  if (legend && legend !== "__auto") {
    return legend.replace(/\{\{\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*\}\}/g, (_, name: string) => labels[name] ?? "");
  }
  const entries = Object.entries(labels).filter(([name]) => name !== "__name__");
  if (entries.length === 0) return labels.__name__ ?? "value";
  return entries.map(([name, value]) => `${name}="${value}"`).join(", ");
}

/** Variable values as the server substitutes them: "All" → `.*`, several → `a|b`. */
export function variableValues(
  variables: DashboardVariable[],
  selected: Record<string, string[]>
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const variable of variables) {
    const values = selected[variable.name] ?? [];
    out[variable.name] =
      values.length === 0 || values.includes(ALL_VALUE)
        ? ".*"
        : values.map((value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|");
  }
  return out;
}

export interface TableRow {
  key: string;
  labels: Record<string, string>;
  values: Record<string, number | null>;
}

/**
 * Joins instant results of a table panel's queries on their labels (Grafana's "merge"): one row
 * per label set, one value column per query.
 */
export function joinTable(
  results: { refId: string; samples: InstantSample[] }[],
  preferredOrder: string[] = []
): {
  labelColumns: string[];
  rows: TableRow[];
} {
  const rows = new Map<string, TableRow>();
  const labelColumns = new Set<string>();
  for (const { refId, samples } of results) {
    for (const sample of samples) {
      const labels = Object.fromEntries(Object.entries(sample.labels).filter(([name]) => name !== "__name__"));
      Object.keys(labels).forEach((name) => labelColumns.add(name));
      const key = Object.keys(labels)
        .sort()
        .map((name) => `${name}=${labels[name]}`)
        .join(",");
      const row = rows.get(key) ?? { key, labels, values: {} };
      row.values[refId] = sample.value;
      rows.set(key, row);
    }
  }
  const rank = (name: string) => {
    const index = preferredOrder.indexOf(name);
    return index < 0 ? preferredOrder.length : index;
  };
  const ordered = [...labelColumns].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
  return { labelColumns: ordered, rows: [...rows.values()] };
}

/** Labels of a query's first `by (…)` clause, in the order written; table columns follow it. */
export function groupingOrder(expr: string | undefined): string[] {
  const match = /\bby\s*\(([^)]*)\)/.exec(expr ?? "");
  return match ? match[1].split(",").map((label) => label.trim()).filter((label) => label && label !== "le") : [];
}

/** Last non-null value of the first series, for stat panels. */
export function lastValue(series: Series[]): number | null {
  const points = series[0]?.points ?? [];
  for (let index = points.length - 1; index >= 0; index--) {
    const value = points[index][1];
    if (value != null) return value;
  }
  return null;
}

/** The colour of the highest threshold `value` reaches. */
export function thresholdColor(thresholds: Threshold[] | undefined, value: number | null): SeriesColor | null {
  if (value == null || !thresholds?.length) return null;
  let color: SeriesColor | null = null;
  for (const threshold of [...thresholds].sort((a, b) => a.value - b.value)) {
    if (value >= threshold.value) color = threshold.color;
  }
  return color;
}

/**
 * Panels as displayed with some rows collapsed: a collapsed row hides the panels up to the next
 * row, and everything below moves up by the space they took.
 */
export function visibleLayout(panels: Panel[], collapsed: ReadonlySet<string>): Panel[] {
  const rows = panels.filter((panel) => panel.type === "row").sort((a, b) => a.gridPos.y - b.gridPos.y);
  if (rows.length === 0 || collapsed.size === 0) return panels;
  const bottom = bottomRow(panels);
  const hidden: { from: number; to: number }[] = [];
  rows.forEach((row, index) => {
    if (!collapsed.has(row.id)) return;
    const next = rows[index + 1]?.gridPos.y ?? bottom;
    if (next > row.gridPos.y + 1) hidden.push({ from: row.gridPos.y + 1, to: next });
  });
  return panels
    .filter((panel) => panel.type === "row" || !hidden.some((h) => panel.gridPos.y >= h.from && panel.gridPos.y < h.to))
    .map((panel) => {
      const shift = hidden.filter((h) => panel.gridPos.y >= h.to).reduce((sum, h) => sum + (h.to - h.from), 0);
      return shift === 0 ? panel : { ...panel, gridPos: { ...panel.gridPos, y: panel.gridPos.y - shift } };
    });
}

export function emptyRow(panels: Panel[]): Panel {
  return { id: newPanelId(), title: "New section", type: "row", gridPos: { x: 0, y: bottomRow(panels), w: GRID_COLUMNS, h: 1 }, targets: [] };
}

/** Next free row below every panel. */
export function bottomRow(panels: Panel[]): number {
  return panels.reduce((max, panel) => Math.max(max, panel.gridPos.y + panel.gridPos.h), 0);
}

function overlaps(a: GridPos, b: GridPos): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/**
 * Settles panels after a move or resize: the moved panel keeps its place, others are pushed
 * down out of its way, then everything floats up as far as it can (Grafana's vertical gravity).
 */
export function compact(panels: Panel[], fixedId?: string): Panel[] {
  const fixed = panels.find((panel) => panel.id === fixedId);
  const ordered = [...panels].sort((a, b) =>
    a.id === fixedId ? -1 : b.id === fixedId ? 1 : a.gridPos.y - b.gridPos.y || a.gridPos.x - b.gridPos.x
  );
  const placed: Panel[] = [];
  for (const panel of ordered) {
    const pos = { ...panel.gridPos, x: Math.min(panel.gridPos.x, GRID_COLUMNS - panel.gridPos.w) };
    if (panel !== fixed) {
      pos.y = 0;
      while (placed.some((other) => overlaps(pos, other.gridPos))) pos.y += 1;
    }
    placed.push({ ...panel, gridPos: pos });
  }
  return placed.sort((a, b) => a.gridPos.y - b.gridPos.y || a.gridPos.x - b.gridPos.x);
}

let counter = 0;

/** Panel ids only need to be unique inside one dashboard. */
export function newPanelId(): string {
  counter += 1;
  return `panel-${Date.now().toString(36)}${counter}`;
}

export function emptyPanel(panels: Panel[]): Panel {
  return {
    id: newPanelId(),
    title: "New panel",
    type: "timeseries",
    gridPos: { x: 0, y: bottomRow(panels), w: 12, h: 8 },
    targets: [{ refId: "A", expr: "", legend: "" }],
    unit: "short",
  };
}

/** Next unused query letter: A, B, … */
export function nextRefId(targets: PanelTarget[]): string {
  for (let code = 65; code < 91; code++) {
    const id = String.fromCharCode(code);
    if (!targets.some((target) => target.refId === id)) return id;
  }
  return `Q${targets.length + 1}`;
}
