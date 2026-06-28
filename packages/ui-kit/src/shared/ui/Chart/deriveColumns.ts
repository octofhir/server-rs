import { getByPath, isFlattenPath } from "./fieldPath";
import type { DerivedField, TabularData } from "./types";

function normalizeScalar(v: unknown): unknown {
    return v === undefined ? null : v;
}

/**
 * Project source data through a set of derived fields, producing a flat
 * `TabularData` whose columns are the field names. Fields whose path flattens
 * an array (`[]`) explode the source row — one output row per array element,
 * zipped by index across all flatten fields; scalar fields repeat.
 */
export function deriveColumns(data: TabularData, fields: DerivedField[]): TabularData {
    if (fields.length === 0) return { columns: [], rows: [] };

    const columns = fields.map((f) => f.name);
    const flat = fields.map((f) => f.array ?? isFlattenPath(f.path));
    const rows: unknown[][] = [];

    for (const raw of data.rows) {
        const obj: Record<string, unknown> = {};
        data.columns.forEach((c, i) => {
            obj[c] = raw[i];
        });

        const values = fields.map((f) => getByPath(obj, f.path));

        // Row count = longest flatten array (min 1).
        let n = 1;
        fields.forEach((_, i) => {
            if (flat[i] && Array.isArray(values[i])) {
                n = Math.max(n, (values[i] as unknown[]).length);
            }
        });

        for (let k = 0; k < n; k++) {
            rows.push(
                fields.map((_, i) => {
                    if (flat[i]) {
                        const arr = values[i];
                        return Array.isArray(arr) ? normalizeScalar(arr[k]) : k === 0 ? normalizeScalar(arr) : null;
                    }
                    return normalizeScalar(values[i]);
                }),
            );
        }
    }

    return { columns, rows };
}
