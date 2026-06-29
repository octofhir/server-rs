// Frontend-orchestrated cell execution. Resolves ${var} templating from the live
// scope, then calls the engine endpoint that already exists for each cell type.
// See docs/ui-notebooks-plan.md §5, §8d.

import type { Cell, Output, Scope } from "./notebook";

const TOKEN = /\$\{([a-zA-Z_][\w.[\]]*)\}/g;

function resolvePath(scope: Scope, path: string): unknown {
  const parts = path.replace(/\[(\d+)\]/g, ".$1").split(".");
  let cur: unknown = scope;
  for (const p of parts) {
    if (cur == null || typeof cur !== "object") return undefined;
    cur = (cur as Record<string, unknown>)[p];
  }
  return cur;
}

/** Substitute `${name}` / `${name.path}` tokens in a string from the scope. */
export function interpolate(src: string, scope: Scope): string {
  return src.replace(TOKEN, (whole, path: string) => {
    const v = resolvePath(scope, path);
    if (v === undefined) return whole;
    return typeof v === "object" ? JSON.stringify(v) : String(v);
  });
}

function deepInterpolate<T>(value: T, scope: Scope): T {
  if (typeof value === "string") return interpolate(value, scope) as unknown as T;
  if (Array.isArray(value)) return value.map((v) => deepInterpolate(v, scope)) as unknown as T;
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value)) out[k] = deepInterpolate(v, scope);
    return out as T;
  }
  return value;
}

async function postJson(url: string, body: unknown): Promise<unknown> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/fhir+json" },
    credentials: "include",
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${text || res.statusText}`);
  }
  return res.json();
}

function errOut(message: string): Output {
  return { kind: "error", severity: "error", message };
}

/** Pull a flat array of scalar results out of a $fhirpath Parameters response. */
function parseFhirPathParams(params: unknown): Output {
  const p = params as { parameter?: Array<Record<string, unknown>> };
  const data: unknown[] = [];
  let meta: { parseTime?: number; evalTime?: number; totalTime?: number } | undefined;
  for (const entry of p.parameter ?? []) {
    if (entry.name === "metadata") {
      const parts = (entry.part as Array<Record<string, unknown>>) ?? [];
      const timing = parts.find((x) => x.name === "timing")?.part as
        | Array<Record<string, unknown>>
        | undefined;
      if (timing) {
        meta = {
          parseTime: Number(timing.find((x) => x.name === "parseTime")?.valueDecimal),
          evalTime: Number(timing.find((x) => x.name === "evaluationTime")?.valueDecimal),
          totalTime: Number(timing.find((x) => x.name === "totalTime")?.valueDecimal),
        };
      }
      continue;
    }
    // value[x] entries
    const valueKey = Object.keys(entry).find((k) => k.startsWith("value"));
    if (valueKey) data.push(entry[valueKey]);
    else if (entry.resource) data.push(entry.resource);
  }
  return { kind: "value", data, meta };
}

/** Decode the $run (SQL-on-FHIR) Parameters response into a table Output. */
function parseSofParams(params: unknown): Output {
  const p = params as { parameter?: Array<Record<string, unknown>> };
  const cols: string[] = [];
  for (const entry of p.parameter ?? []) {
    if (entry.name === "columns") {
      for (const part of (entry.part as Array<Record<string, unknown>>) ?? []) {
        cols.push(String(part.name));
      }
    }
  }
  const rowCountEntry = p.parameter?.find((e) => e.name === "rowCount");
  const rowsEntry = p.parameter?.find((e) => e.name === "rows");
  let rowObjs: Array<Record<string, unknown>> = [];
  const binary = rowsEntry?.resource as { data?: string } | undefined;
  if (binary?.data) {
    try {
      rowObjs = JSON.parse(atob(binary.data));
    } catch {
      rowObjs = [];
    }
  }
  const columns = cols.length ? cols : Object.keys(rowObjs[0] ?? {});
  const rows = rowObjs.map((r) => columns.map((c) => r[c]));
  return {
    kind: "table",
    columns,
    rows,
    meta: {
      rowCount: Number(rowCountEntry?.valueInteger ?? rows.length),
      truncated: false,
    },
  };
}

/** Run a single cell against the live scope. Returns one Output. */
export async function runCell(cell: Cell, scope: Scope): Promise<Output> {
  try {
    switch (cell.type) {
      case "markdown":
        return { kind: "markdown", text: interpolate(cell.source, scope) };

      case "fhirpath": {
        const expr = interpolate(cell.source, scope);
        const body: { resourceType: string; parameter: Array<Record<string, unknown>> } = {
          resourceType: "Parameters",
          parameter: [{ name: "expression", valueString: expr }],
        };
        const ref = cell.config?.contextRef;
        if (ref) {
          // contextRef "ResType/id" → load resource then eval against it
          const r = await fetch(`/fhir/${ref}`, { credentials: "include" });
          if (r.ok) body.parameter.push({ name: "resource", resource: await r.json() });
        }
        return parseFhirPathParams(await postJson("/fhir/$fhirpath", body));
      }

      case "sql": {
        const t0 = performance.now();
        const json = (await postJson("/api/$sql", {
          query: interpolate(cell.source, scope),
          params: cell.config?.params ?? [],
        })) as { columns: string[]; rows: unknown[][]; rowCount: number; executionTimeMs?: number };
        return {
          kind: "table",
          columns: json.columns ?? [],
          rows: json.rows ?? [],
          meta: {
            rowCount: json.rowCount ?? json.rows?.length ?? 0,
            executionTimeMs: json.executionTimeMs ?? Math.round(performance.now() - t0),
            truncated: false,
          },
        };
      }

      case "sql-on-fhir": {
        const view = deepInterpolate(cell.source, scope);
        const body = {
          resourceType: "Parameters",
          parameter: [
            { name: "viewDefinition", resource: view },
            { name: "limit", valueInteger: cell.config?.limit ?? 100 },
          ],
        };
        return parseSofParams(await postJson("/fhir/ViewDefinition/$run", body));
      }

      case "chart": {
        // Chart cells don't execute a query — they render the referenced cell's
        // table output through the spec. Output carries the spec for the renderer.
        return { kind: "chart", spec: cell.source.spec };
      }

      default:
        return errOut(`Unsupported cell type: ${(cell as Cell).type}`);
    }
  } catch (e) {
    return errOut(e instanceof Error ? e.message : String(e));
  }
}
