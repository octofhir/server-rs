import { type Ref, useMemo, useState } from "react";

import { Checkbox } from "../Checkbox";
import { Select } from "../Select";
import { SegmentedRadioGroup } from "../SegmentedRadioGroup";
import { Switch } from "../Switch";
import { TextInput } from "../TextInput";
import { Chart } from "../Chart/Chart";
import { buildChartOption } from "../Chart/buildChartOption";
import type { EChartsType } from "../Chart/echarts";
import { inferColumnType, isNumericType, suggestChartSpec } from "../Chart/inferColumn";
import type { Aggregation, ChartSpec, ChartType, TabularData } from "../Chart/types";
import styles from "./ChartBuilder.module.css";

const TYPE_OPTIONS: { value: ChartType; label: string }[] = [
    { value: "bar", label: "Bar" },
    { value: "line", label: "Line" },
    { value: "area", label: "Area" },
    { value: "pie", label: "Pie" },
    { value: "scatter", label: "Scatter" },
];

const AGG_OPTIONS: { value: Aggregation; label: string }[] = [
    { value: "none", label: "none" },
    { value: "sum", label: "sum" },
    { value: "avg", label: "avg" },
    { value: "count", label: "count" },
    { value: "min", label: "min" },
    { value: "max", label: "max" },
];

export interface ChartBuilderProps {
    data: TabularData;
    /** Controlled spec. When omitted, the builder manages its own state. */
    spec?: ChartSpec;
    onSpecChange?: (spec: ChartSpec) => void;
    /** Preview height in px. Defaults to 360. */
    height?: number;
    /** Forwarded to the inner Chart — gives access to the ECharts instance. */
    chartRef?: Ref<EChartsType | null>;
    className?: string;
}

/**
 * Interactive chart builder: pick chart type, dimensions and measures over any
 * `{columns, rows}` table and see a live brand-colored preview. The resulting
 * `ChartSpec` is serializable and emitted via `onSpecChange`.
 */
export function ChartBuilder({
    data,
    spec: controlledSpec,
    onSpecChange,
    height = 360,
    chartRef,
    className,
}: ChartBuilderProps) {
    const [internalSpec, setInternalSpec] = useState<ChartSpec>(() => suggestChartSpec(data));
    const spec = controlledSpec ?? internalSpec;

    const update = (next: ChartSpec) => {
        if (controlledSpec === undefined) setInternalSpec(next);
        onSpecChange?.(next);
    };

    const columnOptions = useMemo(
        () => data.columns.map((c) => ({ value: c, label: c })),
        [data.columns],
    );

    const numericColumns = useMemo(
        () =>
            data.columns.filter((_, i) => isNumericType(inferColumnType(data.rows, i))),
        [data.columns, data.rows],
    );

    const option = useMemo(() => buildChartOption(spec, data), [spec, data]);

    const hasData = data.columns.length > 0 && data.rows.length > 0;
    const canRender = hasData && spec.series.length > 0 && Boolean(spec.x);

    // --- mutators ---
    const setType = (type: ChartType) => update({ ...spec, type });
    const setX = (x: string | null) => update({ ...spec, x: x ?? undefined });
    const setGroupBy = (g: string | null) => update({ ...spec, groupBy: g ?? undefined });

    const toggleMeasure = (column: string, on: boolean) => {
        const series = on
            ? [...spec.series, { column, agg: "none" as Aggregation }]
            : spec.series.filter((s) => s.column !== column);
        update({ ...spec, series });
    };

    const setMeasureAgg = (column: string, agg: Aggregation) => {
        update({
            ...spec,
            series: spec.series.map((s) => (s.column === column ? { ...s, agg } : s)),
        });
    };

    const measureColumns = numericColumns.length ? numericColumns : data.columns;
    const selected = new Set(spec.series.map((s) => s.column));

    return (
        <div className={className ? `${styles.root} ${className}` : styles.root}>
            <div className={styles.controls}>
                <div className={styles.field}>
                    <span className={styles.label}>Chart type</span>
                    <SegmentedRadioGroup
                        options={TYPE_OPTIONS}
                        value={spec.type}
                        onChange={(v) => setType(v as ChartType)}
                        size="sm"
                        fullWidth
                    />
                </div>

                <div className={styles.field}>
                    <span className={styles.label}>{spec.type === "pie" ? "Category" : "X axis"}</span>
                    <Select
                        data={columnOptions}
                        value={spec.x ?? null}
                        onChange={setX}
                        placeholder="Select column"
                        size="sm"
                    />
                </div>

                <div className={styles.field}>
                    <span className={styles.label}>{spec.type === "scatter" ? "Y / measures" : "Measures"}</span>
                    <div className={styles.measures}>
                        {measureColumns.map((column) => {
                            const on = selected.has(column);
                            const series = spec.series.find((s) => s.column === column);
                            return (
                                <div key={column} className={styles.measureRow}>
                                    <Checkbox
                                        label={column}
                                        checked={on}
                                        onChange={(v) => toggleMeasure(column, v)}
                                    />
                                    {on && spec.type !== "scatter" && (
                                        <Select
                                            data={AGG_OPTIONS}
                                            value={series?.agg ?? "none"}
                                            onChange={(v) => setMeasureAgg(column, (v ?? "none") as Aggregation)}
                                            size="sm"
                                            w={92}
                                        />
                                    )}
                                </div>
                            );
                        })}
                    </div>
                </div>

                {spec.type !== "pie" && spec.type !== "scatter" && (
                    <div className={styles.field}>
                        <span className={styles.label}>Group by</span>
                        <Select
                            data={columnOptions}
                            value={spec.groupBy ?? null}
                            onChange={setGroupBy}
                            placeholder="None"
                            size="sm"
                            clearable
                        />
                    </div>
                )}

                <div className={styles.switches}>
                    {(spec.type === "bar" || spec.type === "area") && (
                        <Switch
                            label="Stack series"
                            checked={spec.stack ?? false}
                            onChange={(v) => update({ ...spec, stack: v })}
                            size="sm"
                        />
                    )}
                    {(spec.type === "line" || spec.type === "area") && (
                        <Switch
                            label="Smooth"
                            checked={spec.smooth ?? false}
                            onChange={(v) => update({ ...spec, smooth: v })}
                            size="sm"
                        />
                    )}
                    {spec.type === "bar" && (
                        <Switch
                            label="Horizontal"
                            checked={spec.horizontal ?? false}
                            onChange={(v) => update({ ...spec, horizontal: v })}
                            size="sm"
                        />
                    )}
                    <Switch
                        label="Legend"
                        checked={spec.legend ?? false}
                        onChange={(v) => update({ ...spec, legend: v })}
                        size="sm"
                    />
                </div>

                <div className={styles.field}>
                    <span className={styles.label}>Title</span>
                    <TextInput
                        value={spec.title ?? ""}
                        onChange={(v) => update({ ...spec, title: v || undefined })}
                        placeholder="Chart title"
                        size="sm"
                    />
                </div>
                {spec.type !== "pie" && (
                    <>
                        <div className={styles.field}>
                            <span className={styles.label}>X label</span>
                            <TextInput
                                value={spec.xLabel ?? ""}
                                onChange={(v) => update({ ...spec, xLabel: v || undefined })}
                                size="sm"
                            />
                        </div>
                        <div className={styles.field}>
                            <span className={styles.label}>Y label</span>
                            <TextInput
                                value={spec.yLabel ?? ""}
                                onChange={(v) => update({ ...spec, yLabel: v || undefined })}
                                size="sm"
                            />
                        </div>
                    </>
                )}
            </div>

            <div className={styles.preview}>
                {canRender ? (
                    <Chart ref={chartRef} option={option} height={height} notMerge />
                ) : (
                    <div className={styles.empty}>
                        {hasData
                            ? "Pick an X dimension and at least one measure to build a chart."
                            : "No data to chart."}
                    </div>
                )}
            </div>
        </div>
    );
}
