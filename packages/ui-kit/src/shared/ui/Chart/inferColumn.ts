import type { ColumnType } from "./types";

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
