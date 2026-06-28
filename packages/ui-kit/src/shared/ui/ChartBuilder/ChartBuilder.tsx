import {
    ChartArea,
    ChartColumnBig,
    ChartLine,
    ChartPie,
    ChartScatter,
} from "lucide-react";
import { type ReactNode, type Ref, useMemo, useState } from "react";

import { Button } from "../Button";
import { Checkbox } from "../Checkbox";
import { NumberInput } from "../NumberInput";
import { Select } from "../Select";
import { SegmentedRadioGroup } from "../SegmentedRadioGroup";
import { Switch } from "../Switch";
import { Tabs } from "../Tabs";
import { TextInput } from "../TextInput";
import { Chart } from "../Chart/Chart";
import { buildChartOption } from "../Chart/buildChartOption";
import { deriveColumns } from "../Chart/deriveColumns";
import { discoverFields } from "../Chart/discoverFields";
import type { EChartsType } from "../Chart/echarts";
import { getByPath, isFlattenPath } from "../Chart/fieldPath";
import { inferColumnType, isNumericType } from "../Chart/inferColumn";
import { suggestChartSpec } from "../Chart/suggest";
import type {
    Aggregation,
    ChartSpec,
    ChartType,
    DateBucket,
    DerivedField,
    FieldDef,
    TabularData,
} from "../Chart/types";
import styles from "./ChartBuilder.module.css";

function typeIcon(label: string, icon: ReactNode): ReactNode {
    return (
        <span role="img" title={label} aria-label={label} style={{ display: "inline-flex" }}>
            {icon}
        </span>
    );
}

const TYPE_OPTIONS: { value: ChartType; label: ReactNode }[] = [
    { value: "bar", label: typeIcon("Bar", <ChartColumnBig size={15} />) },
    { value: "line", label: typeIcon("Line", <ChartLine size={15} />) },
    { value: "area", label: typeIcon("Area", <ChartArea size={15} />) },
    { value: "pie", label: typeIcon("Pie", <ChartPie size={15} />) },
    { value: "scatter", label: typeIcon("Scatter", <ChartScatter size={15} />) },
];

const AGG_OPTIONS: { value: Aggregation; label: string }[] = [
    { value: "none", label: "none" },
    { value: "sum", label: "sum" },
    { value: "avg", label: "avg" },
    { value: "count", label: "count" },
    { value: "min", label: "min" },
    { value: "max", label: "max" },
];

const COUNT_ONLY: { value: Aggregation; label: string }[] = [{ value: "count", label: "count" }];

const BUCKET_OPTIONS: { value: string; label: string }[] = [
    { value: "", label: "Raw" },
    { value: "year", label: "Year" },
    { value: "quarter", label: "Quarter" },
    { value: "month", label: "Month" },
    { value: "week", label: "Week" },
    { value: "day", label: "Day" },
];

/** Synthetic measure — counts rows per category (the BI "count by X" case). */
const COUNT_FIELD: FieldDef = {
    name: "*",
    path: "*",
    label: "Count (rows)",
    type: "int",
    sourceColumn: "*",
    array: false,
};

/** Aggregations valid for a field: numeric → all; everything else → count only. */
function aggOptionsFor(f: FieldDef): { value: Aggregation; label: string }[] {
    if (f.name === COUNT_FIELD.name) return COUNT_ONLY;
    return isNumericType(f.type) ? AGG_OPTIONS : COUNT_ONLY;
}

function rootColumn(path: string): string {
    return path.split(/[.[]/)[0] ?? path;
}

function shortLabel(path: string): string {
    const root = rootColumn(path);
    return path === root ? path : path.slice(root.length + 1) || path;
}

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
 * Interactive chart builder over any `{columns, rows}` table — including JSON
 * cells (e.g. a FHIR `resource` column). Nested fields are auto-discovered into
 * a flat catalog (dot/bracket paths, array `[0]` index + `[]` flatten) so users
 * pick fields without writing paths; an advanced input adds custom paths. The
 * resulting `ChartSpec` is serializable and emitted via `onSpecChange`.
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

    const [activeTab, setActiveTab] = useState("data");
    const [fieldFilter, setFieldFilter] = useState("");
    const [customPath, setCustomPath] = useState("");
    const [customFields, setCustomFields] = useState<FieldDef[]>([]);

    // Auto-discovered field catalog, merged with persisted/custom fields.
    const catalog = useMemo(() => discoverFields(data), [data]);
    const fieldByName = useMemo(() => {
        const map = new Map<string, FieldDef>();
        for (const f of catalog) map.set(f.name, f);
        for (const d of spec.derive ?? []) {
            if (!map.has(d.name)) {
                map.set(d.name, {
                    name: d.name,
                    path: d.path,
                    label: shortLabel(d.path),
                    type: d.type ?? "text",
                    sourceColumn: rootColumn(d.path),
                    array: d.array ?? isFlattenPath(d.path),
                });
            }
        }
        for (const f of customFields) if (!map.has(f.name)) map.set(f.name, f);
        return map;
    }, [catalog, spec.derive, customFields]);

    const allFields = useMemo(() => [...fieldByName.values()], [fieldByName]);
    const fieldOptions = useMemo(
        () => allFields.map((f) => ({ value: f.name, label: f.label })),
        [allFields],
    );

    const resolve = (name: string): FieldDef =>
        fieldByName.get(name) ?? {
            name,
            path: name,
            label: shortLabel(name),
            type: "text",
            sourceColumn: rootColumn(name),
            array: isFlattenPath(name),
        };

    const referencedDerive = (s: ChartSpec): DerivedField[] => {
        const names = new Set<string>();
        if (s.x) names.add(s.x);
        if (s.groupBy) names.add(s.groupBy);
        for (const ser of s.series) names.add(ser.column);
        return [...names].map((n) => {
            const f = resolve(n);
            return { name: f.name, path: f.path, array: f.array, type: f.type };
        });
    };

    const update = (next: ChartSpec) => {
        const withDerive = { ...next, derive: referencedDerive(next) };
        if (controlledSpec === undefined) setInternalSpec(withDerive);
        onSpecChange?.(withDerive);
    };

    // Extract referenced fields, then build the option off the flattened data.
    const effectiveDerive = useMemo<DerivedField[]>(() => {
        if (spec.derive?.length) return spec.derive;
        const names = new Set<string>();
        if (spec.x) names.add(spec.x);
        if (spec.groupBy) names.add(spec.groupBy);
        for (const ser of spec.series) names.add(ser.column);
        return [...names].map((n) => {
            const f = fieldByName.get(n);
            return f
                ? { name: f.name, path: f.path, array: f.array, type: f.type }
                : { name: n, path: n, array: isFlattenPath(n), type: "text" as const };
        });
    }, [spec, fieldByName]);
    const combined = useMemo<TabularData>(
        () => deriveColumns(data, effectiveDerive),
        [data, effectiveDerive],
    );
    const option = useMemo(() => buildChartOption(spec, combined), [spec, combined]);

    const numericFields = allFields.filter((f) => isNumericType(f.type));
    // Scatter needs numeric Y; everything else can count any field, so offer the
    // synthetic "Count (rows)" measure plus all fields — not just numeric ones.
    const measurePool = spec.type === "scatter" ? numericFields : [COUNT_FIELD, ...allFields];
    const measureCandidates = measurePool.filter((f) =>
        f.label.toLowerCase().includes(fieldFilter.toLowerCase()),
    );
    const selected = new Set(spec.series.map((s) => s.column));

    const xIsDate = spec.x ? fieldByName.get(spec.x)?.type === "date" : false;
    const hasData = data.columns.length > 0 && data.rows.length > 0;
    const canRender = hasData && spec.series.length > 0 && Boolean(spec.x);

    // --- mutators ---
    const setType = (type: ChartType) => update({ ...spec, type });
    const setX = (x: string | null) => update({ ...spec, x: x ?? undefined });
    const setGroupBy = (g: string | null) => update({ ...spec, groupBy: g ?? undefined });

    const toggleMeasure = (column: string, on: boolean) => {
        // Default aggregation: sum numeric, count anything else — avoids the
        // "raw value" garbage when a category repeats or the field is text.
        const numeric = column !== COUNT_FIELD.name && isNumericType(fieldByName.get(column)?.type ?? "text");
        const defaultAgg: Aggregation = numeric ? "sum" : "count";
        const series = on
            ? [...spec.series, { column, agg: defaultAgg }]
            : spec.series.filter((s) => s.column !== column);
        update({ ...spec, series });
    };
    const setMeasureAgg = (column: string, agg: Aggregation) => {
        update({
            ...spec,
            series: spec.series.map((s) => (s.column === column ? { ...s, agg } : s)),
        });
    };

    const addCustomPath = () => {
        const path = customPath.trim();
        if (!path || fieldByName.has(path)) {
            setCustomPath("");
            return;
        }
        const samples = data.rows.slice(0, 60).flatMap((row) => {
            const obj: Record<string, unknown> = {};
            data.columns.forEach((c, i) => {
                obj[c] = row[i];
            });
            const v = getByPath(obj, path);
            return Array.isArray(v) ? v : [v];
        });
        const field: FieldDef = {
            name: path,
            path,
            label: shortLabel(path),
            type: inferColumnType(samples.map((v) => [v]), 0),
            sourceColumn: rootColumn(path),
            array: isFlattenPath(path),
        };
        setCustomFields((prev) => [...prev, field]);
        setCustomPath("");
    };

    return (
        <div className={className ? `${styles.root} ${className}` : styles.root}>
            <div className={styles.controls}>
                <Tabs value={activeTab} onChange={(v) => setActiveTab(v ?? "data")}>
                    <Tabs.List>
                        <Tabs.Tab value="data">Data</Tabs.Tab>
                        <Tabs.Tab value="style">Style</Tabs.Tab>
                    </Tabs.List>

                    <Tabs.Panel value="data">
                        <div className={styles.tabBody}>
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
                                <span className={styles.label}>
                                    {spec.type === "pie" ? "Category" : "X axis"}
                                </span>
                                <Select
                                    data={fieldOptions}
                                    value={spec.x ?? null}
                                    onChange={setX}
                                    placeholder="Select field"
                                    size="sm"
                                    searchable
                                />
                            </div>

                            {xIsDate && spec.type !== "scatter" && (
                                <div className={styles.field}>
                                    <span className={styles.label}>Group dates by</span>
                                    <Select
                                        data={BUCKET_OPTIONS}
                                        value={spec.xBucket ?? ""}
                                        onChange={(v) =>
                                            update({ ...spec, xBucket: (v || undefined) as DateBucket | undefined })
                                        }
                                        size="sm"
                                    />
                                </div>
                            )}

                            <div className={styles.field}>
                                <span className={styles.label}>
                                    {spec.type === "scatter" ? "Y / measures" : "Measures"}
                                </span>
                                {allFields.length > 8 && (
                                    <TextInput
                                        value={fieldFilter}
                                        onChange={setFieldFilter}
                                        placeholder="Filter fields…"
                                        size="sm"
                                    />
                                )}
                                <div className={styles.fieldList}>
                                    {measureCandidates.map((f) => {
                                        const on = selected.has(f.name);
                                        const ser = spec.series.find((s) => s.column === f.name);
                                        return (
                                            <div key={f.name} className={styles.fieldRow}>
                                                <span className={styles.fieldMeta}>
                                                    <Checkbox
                                                        label={f.label}
                                                        checked={on}
                                                        onChange={(v) => toggleMeasure(f.name, v)}
                                                    />
                                                    <span
                                                        className={
                                                            f.array
                                                                ? `${styles.typeChip} ${styles.typeChipArray}`
                                                                : styles.typeChip
                                                        }
                                                    >
                                                        {f.array ? "[]" : f.type}
                                                    </span>
                                                </span>
                                                {on && spec.type !== "scatter" && (
                                                    <Select
                                                        data={aggOptionsFor(f)}
                                                        value={ser?.agg ?? "count"}
                                                        onChange={(v) =>
                                                            setMeasureAgg(f.name, (v ?? "none") as Aggregation)
                                                        }
                                                        size="sm"
                                                        w={88}
                                                    />
                                                )}
                                            </div>
                                        );
                                    })}
                                    {measureCandidates.length === 0 && (
                                        <span className={styles.source}>No matching fields.</span>
                                    )}
                                </div>
                            </div>

                            {spec.type !== "pie" && (
                                <div className={styles.field}>
                                    <span className={styles.label}>Y axis</span>
                                    <Switch
                                        label="Start at zero"
                                        checked={spec.yZero ?? spec.type === "bar"}
                                        onChange={(v) => update({ ...spec, yZero: v })}
                                        size="sm"
                                    />
                                    <div className={styles.twoCol}>
                                        <NumberInput
                                            value={spec.yMin ?? null}
                                            onChange={(v) => update({ ...spec, yMin: v ?? undefined })}
                                            placeholder="min (auto)"
                                            size="sm"
                                        />
                                        <NumberInput
                                            value={spec.yMax ?? null}
                                            onChange={(v) => update({ ...spec, yMax: v ?? undefined })}
                                            placeholder="max (auto)"
                                            size="sm"
                                        />
                                    </div>
                                </div>
                            )}

                            {spec.type !== "pie" && spec.type !== "scatter" && (
                                <div className={styles.field}>
                                    <span className={styles.label}>Group by</span>
                                    <Select
                                        data={fieldOptions}
                                        value={spec.groupBy ?? null}
                                        onChange={setGroupBy}
                                        placeholder="None"
                                        size="sm"
                                        searchable
                                        clearable
                                    />
                                </div>
                            )}

                            <div className={styles.field}>
                                <span className={styles.label}>Custom path</span>
                                <div className={styles.advanced}>
                                    <TextInput
                                        value={customPath}
                                        onChange={setCustomPath}
                                        onKeyDown={(e) => {
                                            if (e.key === "Enter") addCustomPath();
                                        }}
                                        placeholder="resource.telecom[].value"
                                        size="sm"
                                    />
                                    <Button size="sm" variant="default" onClick={addCustomPath}>
                                        Add
                                    </Button>
                                </div>
                            </div>
                        </div>
                    </Tabs.Panel>

                    <Tabs.Panel value="style">
                        <div className={styles.tabBody}>
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
                    </Tabs.Panel>
                </Tabs>
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
