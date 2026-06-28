/**
 * Lightweight JSON path extraction — a "micro-DBT" for pulling nested values out
 * of JSON cells (e.g. a FHIR `resource` JSONB column) on the frontend, with no
 * dependency on a full JSONPath / FHIRPath engine.
 *
 * Grammar (dot + bracket):
 *   key            object property            resource.gender
 *   [n]            array index                name[0].family
 *   []             array flatten (explode)    telecom[].value
 *
 * A path containing `[]` yields an array of matched values (flatten); otherwise
 * it yields a single scalar (or null).
 */

export type PathOp =
    | { kind: "key"; name: string }
    | { kind: "index"; index: number }
    | { kind: "flatten" };

const TOKEN = /([^.[\]]+)|\[(\d+)\]|(\[\])/g;

/** Parse a dotted/bracketed path into ops. Invalid fragments are skipped. */
export function parsePath(path: string): PathOp[] {
    const ops: PathOp[] = [];
    let match: RegExpExecArray | null;
    TOKEN.lastIndex = 0;
    // biome-ignore lint/suspicious/noAssignInExpressions: standard regex exec loop
    while ((match = TOKEN.exec(path)) !== null) {
        if (match[1] !== undefined) ops.push({ kind: "key", name: match[1] });
        else if (match[2] !== undefined) ops.push({ kind: "index", index: Number(match[2]) });
        else if (match[3] !== undefined) ops.push({ kind: "flatten" });
    }
    return ops;
}

/** True when a path flattens an array (contains `[]`). */
export function isFlattenPath(path: string): boolean {
    return path.includes("[]");
}

function step(values: unknown[], op: PathOp): { next: unknown[]; flattened: boolean } {
    const next: unknown[] = [];
    let flattened = false;
    for (const v of values) {
        if (op.kind === "key") {
            next.push(v == null ? undefined : (v as Record<string, unknown>)[op.name]);
        } else if (op.kind === "index") {
            next.push(Array.isArray(v) ? v[op.index] : undefined);
        } else {
            if (Array.isArray(v)) {
                flattened = true;
                for (const e of v) next.push(e);
            } else {
                next.push(undefined);
            }
        }
    }
    return { next, flattened };
}

/**
 * Resolve a path against a root value. Returns an array when the path flattens
 * (`[]`), otherwise a single value (null when missing).
 */
export function getByPath(root: unknown, path: string): unknown {
    const ops = parsePath(path);
    let current: unknown[] = [root];
    let flattened = false;
    for (const op of ops) {
        const r = step(current, op);
        current = r.next;
        flattened = flattened || r.flattened;
    }
    if (flattened) return current.map((v) => (v === undefined ? null : v));
    return current.length ? (current[0] ?? null) : null;
}
