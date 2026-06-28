import type { EChartsCoreOption } from "./echarts";
import type { Aggregation, ChartSpec, ChartSeriesSpec, TabularData } from "./types";

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

    const title = spec.title ? { title: { text: spec.title } } : {};
    const legendShown = spec.legend ?? false;
    const legend = legendShown ? { legend: {} } : {};

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
                return { type: "scatter", name: s.name ?? s.column, symbolSize: 10, data: points };
            })
            .filter((s): s is Record<string, unknown> => s !== null);

        return {
            ...title,
            ...legend,
            tooltip: { trigger: "item" },
            xAxis: { type: "value", name: spec.xLabel ?? spec.x },
            yAxis: { type: "value", name: spec.yLabel },
            series,
        };
    }

    // ---- Pie ----
    if (spec.type === "pie") {
        const xi = idx(spec.x);
        const measure = spec.series[0];
        const mi = idx(measure?.column);
        const cats = xi === -1 ? [] : distinct(rows, xi);
        const agg: Aggregation = measure?.agg ?? "sum";

        const pieData = cats.map((cat) => {
            const matching = xi === -1 ? [] : rows.filter((r) => toLabel(r[xi]) === cat);
            const nums =
                mi === -1
                    ? []
                    : matching.map((r) => toNumber(r[mi])).filter((n): n is number => n !== null);
            return { name: cat, value: aggregate(nums, agg, matching.length) };
        });

        return {
            ...title,
            ...legend,
            tooltip: { trigger: "item" },
            series: [{ type: "pie", radius: "70%", name: measure?.name ?? measure?.column, data: pieData }],
        };
    }

    // ---- Cartesian: bar / line / area ----
    const isArea = spec.type === "area";
    const baseType = spec.type === "bar" ? "bar" : "line";
    const xi = idx(spec.x);
    const anyAgg = spec.series.some((s) => (s.agg ?? "none") !== "none") || Boolean(spec.groupBy);

    let categories: string[];
    let seriesList: Record<string, unknown>[];

    const decorate = (s: Record<string, unknown>): Record<string, unknown> => ({
        ...s,
        ...(spec.stack && (baseType === "bar" || isArea) ? { stack: "total" } : {}),
        ...(baseType === "line" && spec.smooth ? { smooth: true } : {}),
        ...(isArea ? { areaStyle: {} } : {}),
    });

    if (spec.groupBy) {
        // Pivot: one series per distinct groupBy value, using series[0] measure.
        const gi = idx(spec.groupBy);
        const measure = spec.series[0];
        const mi = idx(measure?.column);
        const agg: Aggregation = measure?.agg ?? "sum";
        categories = xi === -1 ? [] : distinct(rows, xi);
        const groups = gi === -1 ? [] : distinct(rows, gi);

        seriesList = groups.map((group) => {
            const dataPoints = categories.map((cat) => {
                const matching = rows.filter(
                    (r) => toLabel(r[xi]) === cat && toLabel(r[gi]) === group,
                );
                const nums =
                    mi === -1
                        ? []
                        : matching.map((r) => toNumber(r[mi])).filter((n): n is number => n !== null);
                return aggregate(nums, agg, matching.length);
            });
            return decorate({ type: baseType, name: group, data: dataPoints });
        });
    } else if (anyAgg) {
        categories = xi === -1 ? [] : distinct(rows, xi);
        seriesList = spec.series.map((s: ChartSeriesSpec) => {
            const mi = idx(s.column);
            const agg: Aggregation = s.agg ?? "none";
            const dataPoints = categories.map((cat) => {
                const matching = xi === -1 ? [] : rows.filter((r) => toLabel(r[xi]) === cat);
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
        categories = xi === -1 ? rows.map((_, i) => String(i)) : rows.map((r) => toLabel(r[xi]));
        seriesList = spec.series.map((s: ChartSeriesSpec) => {
            const mi = idx(s.column);
            const dataPoints = mi === -1 ? [] : rows.map((r) => toNumber(r[mi]) ?? 0);
            return decorate({ type: baseType, name: s.name ?? s.column, data: dataPoints });
        });
    }

    const categoryAxis = {
        type: "category" as const,
        data: categories,
        name: spec.horizontal ? spec.yLabel : spec.xLabel ?? spec.x,
        boundaryGap: baseType === "bar",
    };
    const valueAxis = {
        type: "value" as const,
        name: spec.horizontal ? spec.xLabel ?? spec.x : spec.yLabel,
    };

    return {
        ...title,
        ...legend,
        tooltip: { trigger: "axis" },
        xAxis: spec.horizontal ? valueAxis : categoryAxis,
        yAxis: spec.horizontal ? categoryAxis : valueAxis,
        series: seriesList,
    };
}
