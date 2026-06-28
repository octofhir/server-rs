import type { ChartSpec, ColumnType, TabularData } from "./types";

/**
 * Infer a lightweight column type hint from the first non-null sample value.
 * Ported from `ui/src/pages/db-console/components/resultExport.ts`.
 */
export function inferColumnType(rows: unknown[][], columnIndex: number): ColumnType {
    for (const row of rows) {
        const value = row[columnIndex];
        if (value === null || value === undefined) continue;
        if (typeof value === "number") return Number.isInteger(value) ? "int" : "num";
        if (typeof value === "boolean") return "bool";
        if (typeof value === "object") return "json";
        const text = String(value);
        if (/^\d{4}-\d{2}-\d{2}([T\s]\d{2}:\d{2})?/.test(text)) return "date";
        if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-/i.test(text)) return "uuid";
        return "text";
    }
    return "null";
}

const NUMERIC: ReadonlySet<ColumnType> = new Set(["int", "num"]);

/** True when a column hint is numeric (a candidate measure). */
export function isNumericType(type: ColumnType): boolean {
    return NUMERIC.has(type);
}

/**
 * Suggest a reasonable starting chart spec for arbitrary tabular data.
 * Picks a dimension (first date/text/categorical column) and the numeric
 * columns as measures; falls back to bar/line/scatter based on shapes found.
 */
export function suggestChartSpec(data: TabularData): ChartSpec {
    const { columns, rows } = data;
    const types = columns.map((_, i) => inferColumnType(rows, i));

    const numericCols = columns.filter((_, i) => isNumericType(types[i]));
    const dateIdx = types.findIndex((t) => t === "date");
    const textIdx = types.findIndex((t) => t === "text" || t === "bool");

    // No numeric columns at all → nothing meaningful; default to a bar on the
    // first column counted.
    if (numericCols.length === 0) {
        return {
            type: "bar",
            x: columns[0],
            series: columns[1] ? [{ column: columns[1], agg: "count" }] : [],
            legend: false,
        };
    }

    // Two-or-more numeric columns and no obvious dimension → scatter.
    if (numericCols.length >= 2 && dateIdx === -1 && textIdx === -1) {
        return {
            type: "scatter",
            x: numericCols[0],
            series: [{ column: numericCols[1] }],
            legend: false,
        };
    }

    // Time series → line; otherwise categorical bar.
    const xCol = dateIdx !== -1 ? columns[dateIdx] : textIdx !== -1 ? columns[textIdx] : columns[0];
    const measures = numericCols.filter((c) => c !== xCol).slice(0, 4);

    return {
        type: dateIdx !== -1 ? "line" : "bar",
        x: xCol,
        series: (measures.length ? measures : numericCols.slice(0, 1)).map((column) => ({
            column,
            agg: "none",
        })),
        legend: measures.length > 1,
    };
}
