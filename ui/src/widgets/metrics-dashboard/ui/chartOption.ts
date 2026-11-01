import { type ColorScheme, type EChartsCoreOption, palette } from "@octofhir/ui-kit";
import { formatAxis, formatValue, type Panel, type RangeResult, type SeriesColor, seriesName } from "@/entities/metrics";

/** Semantic series colours per scheme. */
const SEMANTIC: Record<ColorScheme, Record<SeriesColor, string>> = {
  light: {
    neutral: palette.gray[5],
    accent: palette.primary[6],
    success: palette.success[6],
    warning: palette.warning[5],
    error: palette.error[6],
    info: palette.info[6],
  },
  dark: {
    neutral: palette.gray[4],
    accent: palette.primary[5],
    success: palette.success[5],
    warning: palette.warning[5],
    error: palette.error[5],
    info: palette.info[5],
  },
};

/**
 * Categorical order for series without a fixed colour. Warning and error hues come last so an
 * ordinary series is never mistaken for a problem.
 */
const CATEGORICAL: Record<ColorScheme, string[]> = {
  light: [
    palette.primary[6],
    palette.info[6],
    palette.accent[6],
    palette.success[6],
    palette.deep[6],
    palette.primary[8],
    palette.info[8],
    palette.accent[8],
    palette.success[8],
    palette.gray[6],
  ],
  dark: [
    palette.primary[5],
    palette.info[4],
    palette.accent[4],
    palette.success[4],
    palette.deep[4],
    palette.primary[3],
    palette.info[2],
    palette.accent[2],
    palette.success[2],
    palette.gray[3],
  ],
};

export function semanticColor(color: SeriesColor, scheme: ColorScheme): string {
  return SEMANTIC[scheme][color];
}

/** One drawn series with what the legend shows about it. */
export interface DrawnSeries {
  key: string;
  name: string;
  color: string;
  points: [number, number | null][];
  last: number | null;
  mean: number | null;
  max: number | null;
}

/** Series of a timeseries panel, named by legend template and coloured. */
export function drawnSeries(
  panel: Panel,
  results: { refId: string; result: RangeResult }[],
  scheme: ColorScheme
): DrawnSeries[] {
  let next = 0;
  return results.flatMap(({ refId, result }) => {
    const target = panel.targets.find((t) => t.refId === refId);
    return result.series.map((series, index) => {
      const name = seriesName(target?.legend, series.labels);
      const fixed = panel.colors?.[name];
      const color = fixed ? SEMANTIC[scheme][fixed] : CATEGORICAL[scheme][next++ % CATEGORICAL[scheme].length];
      const values = series.points.map(([, v]) => v).filter((v): v is number => v != null);
      let last: number | null = null;
      for (let i = series.points.length - 1; i >= 0; i--) {
        if (series.points[i][1] != null) {
          last = series.points[i][1];
          break;
        }
      }
      return {
        key: `${refId}:${index}:${name}`,
        name,
        color,
        points: series.points,
        last,
        mean: values.length ? values.reduce((a, b) => a + b, 0) / values.length : null,
        max: values.length ? Math.max(...values) : null,
      };
    });
  });
}

function clock(value: number, spanMs: number): string {
  const date = new Date(value);
  if (spanMs > 2 * 86_400_000) {
    return date.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false });
  }
  if (spanMs <= 10 * 60_000) {
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
  }
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
}

/** A point with no neighbours draws nothing as a line; give it a dot instead. */
function isolatedSizes(points: [number, number | null][]): number[] {
  return points.map(([, value], index) =>
    value != null && points[index - 1]?.[1] == null && points[index + 1]?.[1] == null ? 4 : 0
  );
}

/** ECharts option for a timeseries panel. `hidden` lists series keys switched off in the legend. */
export function timeseriesOption(
  panel: Panel,
  series: DrawnSeries[],
  range: { from: number; to: number },
  hidden: ReadonlySet<string>,
  options: { zoom?: boolean; showTooltip?: boolean } = {}
): EChartsCoreOption {
  const bars = panel.drawStyle === "bars";
  const fill = !bars && !panel.stack && series.length === 1;
  const span = range.to - range.from;
  const shown = series.filter((s) => !hidden.has(s.key));
  return {
    animation: false,
    grid: { left: 4, right: 8, top: 8, bottom: 4, containLabel: true },
    legend: { show: false },
    toolbox: options.zoom
      ? {
          // Invisible: only drives drag-to-select zoom, which the panel turns into a time range.
          show: true,
          itemSize: 0,
          right: -100,
          feature: { dataZoom: { yAxisIndex: "none", brushStyle: { color: palette.gray[5], opacity: 0.15 } } },
        }
      : undefined,
    tooltip: {
      trigger: "axis",
      // Connected charts all follow the cursor; only the hovered one shows the values box.
      showContent: options.showTooltip ?? true,
      confine: true,
      order: "valueDesc",
      axisPointer: { type: "line", lineStyle: { type: "dashed", width: 1 } },
      valueFormatter: (value: unknown) =>
        typeof value === "number" ? formatValue(value, panel.unit, panel.decimals) : "—",
    },
    xAxis: {
      type: "time",
      min: range.from,
      max: range.to,
      splitLine: { show: false },
      axisTick: { show: false },
      axisLabel: { formatter: (value: number) => clock(value, span), hideOverlap: true },
    },
    yAxis: {
      type: "value",
      min: panel.min,
      max: panel.max,
      splitNumber: 3,
      axisLabel: { formatter: (value: number) => formatAxis(value, panel.unit) },
    },
    series: shown.map((s) => {
      const sizes = isolatedSizes(s.points);
      return {
        id: s.key,
        name: s.name,
        type: bars ? "bar" : "line",
        stack: panel.stack ? "total" : undefined,
        showSymbol: !bars && sizes.some((size) => size > 0),
        symbol: "circle",
        symbolSize: (_: unknown, params: { dataIndex: number }) => sizes[params.dataIndex] ?? 0,
        connectNulls: false,
        barMaxWidth: 10,
        barCategoryGap: "15%",
        lineStyle: bars ? undefined : { width: 1.5, color: s.color },
        itemStyle: { color: s.color },
        areaStyle: fill || (panel.stack && !bars) ? { color: s.color, opacity: panel.stack ? 0.35 : 0.08 } : undefined,
        emphasis: { disabled: true },
        data: s.points,
      };
    }),
  };
}

/** A minimal filled trend for stat panels. */
export function statTrendOption(points: [number, number | null][], color: string): EChartsCoreOption {
  return {
    animation: false,
    grid: { left: 0, right: 0, top: 2, bottom: 0 },
    xAxis: { type: "time", show: false },
    yAxis: { type: "value", show: false, min: 0 },
    tooltip: { show: false },
    series: [
      {
        type: "line",
        showSymbol: false,
        connectNulls: false,
        lineStyle: { width: 1.25, color },
        areaStyle: { color, opacity: 0.12 },
        silent: true,
        data: points,
      },
    ],
  };
}
