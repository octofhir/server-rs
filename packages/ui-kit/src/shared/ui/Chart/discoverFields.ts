import { getByPath } from "./fieldPath";
import { inferColumnType } from "./inferColumn";
import type { ColumnType, FieldDef, TabularData } from "./types";

export interface DiscoverOptions {
    /** Rows sampled to infer structure + types. Default 60. */
    sampleRows?: number;
    /** Max nesting depth walked into JSON. Default 5. */
    maxDepth?: number;
    /** Hard cap on emitted fields. Default 250. */
    maxFields?: number;
}

/** Short label for a path — drops a leading `<column>.` prefix when redundant. */
function labelFor(path: string, sourceColumn: string): string {
    return path === sourceColumn ? path : path.slice(sourceColumn.length + 1) || path;
}

/** Infer a column type from a flat list of sample values. */
function inferType(values: unknown[]): ColumnType {
    return inferColumnType(
        values.map((v) => [v]),
        0,
    );
}

/**
 * Discover a flat catalog of chartable fields from tabular data. Plain columns
 * pass through as-is; JSON-object columns are walked into dotted/bracket paths.
 * Each array leaf yields both an index form (`[0]`, scalar) and a flatten twin
 * (`[]`, explodes rows), giving both "pick one" and "distribution" behaviors.
 */
export function discoverFields(data: TabularData, opts: DiscoverOptions = {}): FieldDef[] {
    const { sampleRows = 60, maxDepth = 5, maxFields = 250 } = opts;
    const sample = data.rows.slice(0, sampleRows);
    const out = new Map<string, FieldDef>();

    const rowObjects = sample.map((row) => {
        const obj: Record<string, unknown> = {};
        data.columns.forEach((c, i) => {
            obj[c] = row[i];
        });
        return obj;
    });

    const emit = (path: string, sourceColumn: string, array: boolean) => {
        if (out.has(path) || out.size >= maxFields) return;
        const values = rowObjects.flatMap((obj) => {
            const v = getByPath(obj, path);
            return Array.isArray(v) ? v : [v];
        });
        out.set(path, {
            name: path,
            path,
            label: labelFor(path, sourceColumn),
            type: inferType(values),
            sourceColumn,
            array,
        });
    };

    const emitLeaf = (path: string, sourceColumn: string) => {
        emit(path, sourceColumn, false);
        // Flatten twin: replace the last `[0]` index with `[]`.
        const idx = path.lastIndexOf("[0]");
        if (idx !== -1) {
            const twin = `${path.slice(0, idx)}[]${path.slice(idx + 3)}`;
            emit(twin, sourceColumn, true);
        }
    };

    const walk = (value: unknown, prefix: string, sourceColumn: string, depth: number) => {
        if (out.size >= maxFields) return;
        if (Array.isArray(value)) {
            if (depth >= maxDepth) return;
            walk(value[0], `${prefix}[0]`, sourceColumn, depth + 1);
        } else if (value !== null && typeof value === "object") {
            if (depth >= maxDepth) {
                emitLeaf(prefix, sourceColumn);
                return;
            }
            for (const key of Object.keys(value as Record<string, unknown>)) {
                walk((value as Record<string, unknown>)[key], `${prefix}.${key}`, sourceColumn, depth + 1);
            }
        } else {
            emitLeaf(prefix, sourceColumn);
        }
    };

    data.columns.forEach((col, ci) => {
        const firstObj = sample.find((r) => {
            const cell = r[ci];
            return cell !== null && typeof cell === "object";
        });
        if (!firstObj) {
            // Plain scalar column.
            const values = sample.map((r) => r[ci]);
            out.set(col, {
                name: col,
                path: col,
                label: col,
                type: inferType(values),
                sourceColumn: col,
                array: false,
            });
            return;
        }
        // JSON column: walk every sampled object cell to union keys.
        for (const r of sample) {
            const cell = r[ci];
            if (cell !== null && typeof cell === "object") walk(cell, col, col, 0);
            if (out.size >= maxFields) break;
        }
    });

    return [...out.values()];
}
