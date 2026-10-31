import { type ColorScheme, type EChartsCoreOption, palette } from "@octofhir/ui-kit";
import type { IntervalPoint, QuantilePoint } from "../lib/model";
import type { StatusGroup } from "../lib/prometheus";
import { formatAxisMs, formatMs, formatRate } from "./format";

/**
 * Series colors, checked with the dataviz palette validator against both chart surfaces.
 * OK traffic stays neutral so client and server errors are what stands out; the latency
 * quantiles are one Marigold ramp that gets heavier towards p99.
 */
const COLORS: Record<ColorScheme, { groups: Record<StatusGroup, string>; quantiles: [string, string, string] }> = {
  light: {
    groups: { ok: palette.gray[5], client: palette.warning[5], server: palette.error[6] },
    quantiles: [palette.primary[6], palette.primary[7], palette.primary[8]],
  },
  dark: {
    groups: { ok: palette.gray[4], client: palette.warning[5], server: palette.error[6] },
    quantiles: [palette.primary[8], palette.primary[6], palette.primary[2]],
  },
};

export const GROUP_LABELS: Record<StatusGroup, string> = { ok: "OK", client: "4xx", server: "5xx" };

const GROUP_ORDER: StatusGroup[] = ["ok", "client", "server"];

function clock(value: number): string {
  return new Date(value).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}

function base(
  range: { from: number; to: number },
  unit: (value: number) => string,
  axisUnit: (value: number) => string
) {
  return {
    animation: false,
    grid: { left: 8, right: 16, top: 32, bottom: 4 },
    legend: { top: 0, left: 0, icon: "roundRect", itemWidth: 10, itemHeight: 10, itemGap: 14 },
    tooltip: { trigger: "axis", valueFormatter: (value: unknown) => (typeof value === "number" ? unit(value) : "—") },
    xAxis: {
      type: "time",
      min: range.from,
      max: range.to,
      splitLine: { show: false },
      axisLabel: { formatter: (value: number) => clock(value), hideOverlap: true },
    },
    yAxis: {
      type: "value",
      splitNumber: 3,
      axisLabel: { formatter: axisUnit },
    },
  };
}

/** Requests per second, stacked by response group. */
export function throughputOption(
  points: IntervalPoint[],
  range: { from: number; to: number },
  scheme: ColorScheme
): EChartsCoreOption {
  const colors = COLORS[scheme].groups;
  return {
    ...base(range, formatRate, (value) => value.toLocaleString()),
    series: GROUP_ORDER.map((group) => ({
      name: GROUP_LABELS[group],
      type: "bar",
      stack: "rate",
      barMaxWidth: 6,
      barCategoryGap: "20%",
      itemStyle: { color: colors[group] },
      emphasis: { disabled: true },
      data: points.map((point) => [point.at, point.rate ? point.rate[group] : null]),
    })),
  };
}

const LINE = { type: "line", showSymbol: false, symbol: "circle", symbolSize: 8, connectNulls: false } as const;

/** Mean latency of all requests per scrape interval. */
export function meanLatencyOption(
  points: IntervalPoint[],
  range: { from: number; to: number },
  scheme: ColorScheme
): EChartsCoreOption {
  return {
    ...base(range, formatMs, formatAxisMs),
    legend: { show: false },
    series: [
      {
        ...LINE,
        name: "Mean",
        lineStyle: { width: 2, color: COLORS[scheme].quantiles[1] },
        itemStyle: { color: COLORS[scheme].quantiles[1] },
        data: points.map((point) => [point.at, point.meanMs]),
      },
    ],
  };
}

/** One route's rolling p50 / p95 / p99. */
export function quantileOption(
  points: QuantilePoint[],
  range: { from: number; to: number },
  scheme: ColorScheme
): EChartsCoreOption {
  const keys = ["p50", "p95", "p99"] as const;
  return {
    ...base(range, formatMs, formatAxisMs),
    series: keys.map((key, index) => ({
      ...LINE,
      name: key,
      lineStyle: { width: 2, color: COLORS[scheme].quantiles[index] },
      itemStyle: { color: COLORS[scheme].quantiles[index] },
      data: points.map((point) => [point.at, point[key]]),
    })),
  };
}
