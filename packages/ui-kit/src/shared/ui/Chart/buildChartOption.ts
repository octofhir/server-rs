import type { EChartsCoreOption } from "./echarts";
import type {
  Aggregation,
  ChartPalette,
  ChartSeriesSpec,
  ChartSpec,
  DateBucket,
  TabularData,
} from "./types";

/** Categorical color palettes. `theme` defers to the registered ECharts theme. */
const PALETTES: Record<Exclude<ChartPalette, "theme">, string[]> = {
  vibrant: ["#6366f1", "#ec4899", "#f59e0b", "#10b981", "#06b6d4", "#8b5cf6", "#ef4444", "#84cc16"],
  cool: ["#0ea5e9", "#6366f1", "#14b8a6", "#8b5cf6", "#22d3ee", "#3b82f6", "#a855f7", "#2dd4bf"],
  warm: ["#f97316", "#ef4444", "#f59e0b", "#e11d48", "#fb7185", "#facc15", "#fdba74", "#dc2626"],
  sunset: ["#7c3aed", "#db2777", "#f97316", "#facc15", "#fb7185", "#c026d3", "#fca5a5", "#fde047"],
  mono: ["#1e293b", "#334155", "#475569", "#64748b", "#94a3b8", "#cbd5e1", "#0f172a", "#e2e8f0"],
};

function paletteColor(palette?: ChartPalette): { color?: string[] } {
  if (!palette || palette === "theme") return {};
  return { color: PALETTES[palette] };
}

/** Coerce an unknown cell to a finite number, or null. */
function toNumber(value: unknown): number | null {
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (typeof value === "string" && value.trim() !== "") {
    const n = Number(value);
    return Number.isFinite(n) ? n : null;
  }
  return null;
}

/** Stringify a cell for use as a category label. */
function toLabel(value: unknown): string {
  if (value === null || value === undefined) return "∅";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

const pad = (n: number): string => String(n).padStart(2, "0");

/**
 * Truncate a date-ish cell to a calendar bucket label (sortable: year,
 * quarter, month, week, day). Falls back to the raw label on unparseable
 * values. Uses UTC to avoid timezone drift.
 */
function bucketDate(value: unknown, granularity: DateBucket): string {
  if (value === null || value === undefined) return "∅";
  const date = new Date(typeof value === "number" ? value : String(value));
  if (Number.isNaN(date.getTime())) return toLabel(value);
  const y = date.getUTCFullYear();
  const m = date.getUTCMonth() + 1;
  const d = date.getUTCDate();
  switch (granularity) {
    case "year":
      return String(y);
    case "quarter":
      return `${y}-Q${Math.floor((m - 1) / 3) + 1}`;
    case "month":
      return `${y}-${pad(m)}`;
    case "week": {
      // ISO-8601 week number.
      const t = new Date(Date.UTC(y, date.getUTCMonth(), d));
      const dayNum = (t.getUTCDay() + 6) % 7;
      t.setUTCDate(t.getUTCDate() - dayNum + 3);
      const firstThursday = new Date(Date.UTC(t.getUTCFullYear(), 0, 4));
      const week =
        1 +
        Math.round(
          ((t.getTime() - firstThursday.getTime()) / 86_400_000 -
            3 +
            ((firstThursday.getUTCDay() + 6) % 7)) /
            7
        );
      return `${t.getUTCFullYear()}-W${pad(week)}`;
    }
    default:
      return `${y}-${pad(m)}-${pad(d)}`;
  }
}

function aggregate(values: number[], agg: Aggregation, rawCount: number): number {
  switch (agg) {
    case "count":
      return rawCount;
    case "sum":
      return values.reduce((a, b) => a + b, 0);
    case "avg":
      return values.length ? values.reduce((a, b) => a + b, 0) / values.length : 0;
    case "min":
      return values.length ? Math.min(...values) : 0;
    case "max":
      return values.length ? Math.max(...values) : 0;
    default:
      return values.length ? values[0] : 0;
  }
}

/** Distinct values of a column, in first-seen order. */
function distinct(rows: unknown[][], idx: number): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const row of rows) {
    const label = toLabel(row[idx]);
    if (!seen.has(label)) {
      seen.add(label);
      out.push(label);
    }
  }
  return out;
}

/**
 * Build an ECharts option from a chart spec + tabular data. Pure and defensive:
 * unknown columns degrade to empty series rather than throwing. Applies NO
 * colors — the brand palette comes from the registered theme.
 */
export function buildChartOption(spec: ChartSpec, data: TabularData): EChartsCoreOption {
  const { columns, rows } = data;
  const idx = (name?: string) => (name ? columns.indexOf(name) : -1);

  // X label — bucketed when `xBucket` is set (date truncation), else raw.
  const xLabel = (cell: unknown): string =>
    spec.xBucket ? bucketDate(cell, spec.xBucket) : toLabel(cell);
  const distinctX = (xi: number): string[] => {
    const seen = new Set<string>();
    const out: string[] = [];
    for (const row of rows) {
      const label = xLabel(row[xi]);
      if (!seen.has(label)) {
        seen.add(label);
        out.push(label);
      }
    }
    if (spec.xBucket) out.sort();
    return out;
  };

  const title = spec.title ? { title: { text: spec.title } } : {};
  const legendShown = spec.legend ?? false;
  const legend = legendShown ? { legend: {} } : {};
  const color = paletteColor(spec.palette);
  const showLabels = spec.showLabels ?? false;
  const pointLabel = showLabels ? { label: { show: true, position: "top" } } : {};

  // ---- Scatter ----
  if (spec.type === "scatter") {
    const xi = idx(spec.x);
    const series = spec.series
      .map((s): Record<string, unknown> | null => {
        const yi = idx(s.column);
        if (xi === -1 || yi === -1) return null;
        const points = rows
          .map((r) => [toNumber(r[xi]), toNumber(r[yi])])
          .filter((p): p is [number, number] => p[0] !== null && p[1] !== null);
        return {
          type: "scatter",
          name: s.name ?? s.column,
          symbolSize: 10,
          data: points,
          ...pointLabel,
        };
      })
      .filter((s): s is Record<string, unknown> => s !== null);

    return {
      ...title,
      ...legend,
      ...color,
      tooltip: { trigger: "item" },
      xAxis: { type: "value", name: spec.xLabel ?? spec.x, scale: true },
      yAxis: {
        type: "value",
        name: spec.yLabel,
        scale: !(spec.yZero ?? false),
        ...(spec.yMin != null ? { min: spec.yMin } : {}),
        ...(spec.yMax != null ? { max: spec.yMax } : {}),
      },
      series,
    };
  }

  // ---- Pie ----
  if (spec.type === "pie") {
    const xi = idx(spec.x);
    const measure = spec.series[0];
    const mi = idx(measure?.column);
    const cats = xi === -1 ? [] : distinctX(xi);
    const agg: Aggregation = measure?.agg ?? "sum";

    const pieData = cats.map((cat) => {
      const matching = xi === -1 ? [] : rows.filter((r) => xLabel(r[xi]) === cat);
      const nums =
        mi === -1
          ? []
          : matching.map((r) => toNumber(r[mi])).filter((n): n is number => n !== null);
      return { name: cat, value: aggregate(nums, agg, matching.length) };
    });

    return {
      ...title,
      ...legend,
      ...color,
      tooltip: { trigger: "item" },
      series: [
        {
          type: "pie",
          radius: spec.donut ? ["45%", "70%"] : "70%",
          name: measure?.name ?? measure?.column,
          data: pieData,
          label: showLabels ? { show: true, formatter: "{b}: {c}" } : { show: true },
        },
      ],
    };
  }

  // ---- Cartesian: bar / line / area ----
  const isArea = spec.type === "area";
  const baseType = spec.type === "bar" ? "bar" : "line";
  const xi = idx(spec.x);
  const anyAgg = spec.series.some((s) => (s.agg ?? "none") !== "none") || Boolean(spec.groupBy);

  let categories: string[];
  let seriesList: Record<string, unknown>[];

  const labelPos = spec.horizontal ? "right" : "top";
  const decorate = (s: Record<string, unknown>): Record<string, unknown> => ({
    ...s,
    ...(spec.stack && (baseType === "bar" || isArea) ? { stack: "total" } : {}),
    ...(baseType === "line" && spec.smooth ? { smooth: true } : {}),
    ...(isArea ? { areaStyle: {} } : {}),
    ...(showLabels ? { label: { show: true, position: labelPos } } : {}),
  });

  if (spec.groupBy) {
    // Pivot: one series per distinct groupBy value, using series[0] measure.
    const gi = idx(spec.groupBy);
    const measure = spec.series[0];
    const mi = idx(measure?.column);
    const agg: Aggregation = measure?.agg ?? "sum";
    categories = xi === -1 ? [] : distinctX(xi);
    const groups = gi === -1 ? [] : distinct(rows, gi);

    seriesList = groups.map((group) => {
      const dataPoints = categories.map((cat) => {
        const matching = rows.filter((r) => xLabel(r[xi]) === cat && toLabel(r[gi]) === group);
        const nums =
          mi === -1
            ? []
            : matching.map((r) => toNumber(r[mi])).filter((n): n is number => n !== null);
        return aggregate(nums, agg, matching.length);
      });
      return decorate({ type: baseType, name: group, data: dataPoints });
    });
  } else if (anyAgg) {
    categories = xi === -1 ? [] : distinctX(xi);
    seriesList = spec.series.map((s: ChartSeriesSpec) => {
      const mi = idx(s.column);
      const agg: Aggregation = s.agg ?? "none";
      const dataPoints = categories.map((cat) => {
        const matching = xi === -1 ? [] : rows.filter((r) => xLabel(r[xi]) === cat);
        const nums =
          mi === -1
            ? []
            : matching.map((r) => toNumber(r[mi])).filter((n): n is number => n !== null);
        return aggregate(nums, agg, matching.length);
      });
      return decorate({ type: baseType, name: s.name ?? s.column, data: dataPoints });
    });
  } else {
    // Raw rows, no aggregation.
    categories = xi === -1 ? rows.map((_, i) => String(i)) : rows.map((r) => xLabel(r[xi]));
    seriesList = spec.series.map((s: ChartSeriesSpec) => {
      const mi = idx(s.column);
      const dataPoints = mi === -1 ? [] : rows.map((r) => toNumber(r[mi]) ?? 0);
      return decorate({ type: baseType, name: s.name ?? s.column, data: dataPoints });
    });
  }

  const categoryAxis = {
    type: "category" as const,
    data: categories,
    name: spec.horizontal ? spec.yLabel : (spec.xLabel ?? spec.x),
    boundaryGap: baseType === "bar",
  };
  // Bars read best anchored at zero; lines/areas zoom to the data range
  // unless the user pins it. `scale: true` lets the axis skip zero.
  const includeZero = spec.yZero ?? baseType === "bar";
  const valueAxis = {
    type: "value" as const,
    name: spec.horizontal ? (spec.xLabel ?? spec.x) : spec.yLabel,
    scale: !includeZero,
    ...(spec.yMin != null ? { min: spec.yMin } : {}),
    ...(spec.yMax != null ? { max: spec.yMax } : {}),
  };

  return {
    ...title,
    ...legend,
    ...color,
    tooltip: { trigger: "axis" },
    xAxis: spec.horizontal ? valueAxis : categoryAxis,
    yAxis: spec.horizontal ? categoryAxis : valueAxis,
    series: seriesList,
  };
}
