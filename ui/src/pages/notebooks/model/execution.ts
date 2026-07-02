// Frontend-orchestrated cell execution. Resolves ${var} templating from the live
// scope, then calls the engine endpoint that already exists for each cell type.
// See docs/ui-notebooks-plan.md §5, §8d.

import type { Cell, Output, Scope } from "./notebook";
import { runPipeline } from "./pipeline";

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

/** Decode a $cql Parameters response into a json (library) or value (expression) Output. */
function parseCqlParams(params: unknown): Output {
  const p = params as { parameter?: Array<Record<string, unknown>> };
  const parts = p.parameter ?? [];
  const tryJson = (s: string): unknown => {
    try {
      return JSON.parse(s);
    } catch {
      return s;
    }
  };
  const result = parts.find((x) => x.name === "result");
  if (Array.isArray(result?.part)) {
    const defines: Record<string, unknown> = {};
    for (const part of result.part as Array<Record<string, unknown>>) {
      if (part.valueString !== undefined)
        defines[String(part.name)] = tryJson(String(part.valueString));
    }
    return { kind: "json", data: defines };
  }
  const ret = parts.find((x) => x.name === "return")?.valueString;
  return { kind: "value", data: [ret !== undefined ? tryJson(String(ret)) : null] };
}

const CQL_LIBRARY = /(^|\n)\s*(library|using|define)\b/;

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

      case "cql": {
        const src = interpolate(cell.source, scope);
        const isLib = CQL_LIBRARY.test(src);
        const parameter: Array<Record<string, unknown>> = [
          isLib ? { name: "library", valueString: src } : { name: "expression", valueString: src },
        ];
        if (cell.config?.context) {
          parameter.push({ name: "context", valueString: cell.config.context });
        }
        const body = { resourceType: "Parameters", parameter };
        return parseCqlParams(await postJson("/fhir/$cql", body));
      }

      case "graphql": {
        const res = await fetch("/$graphql", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          credentials: "include",
          body: JSON.stringify({
            query: interpolate(cell.source, scope),
            variables: deepInterpolate(cell.config?.variables ?? {}, scope),
          }),
        });
        const json = (await res.json()) as { data?: unknown; errors?: Array<{ message: string }> };
        if (json.errors?.length) {
          return {
            kind: "error",
            severity: "error",
            message: json.errors.map((e) => e.message).join("; "),
          };
        }
        return { kind: "json", data: json.data };
      }

      case "rest": {
        const method = cell.source.method;
        const rawUrl = interpolate(cell.source.url, scope);
        const url = rawUrl.startsWith("http")
          ? rawUrl
          : `/fhir${rawUrl.startsWith("/") ? "" : "/"}${rawUrl}`;
        const init: RequestInit = {
          method,
          credentials: "include",
          headers: { Accept: "application/fhir+json", ...(cell.source.headers ?? {}) },
        };
        if (method !== "GET" && cell.source.body != null) {
          (init.headers as Record<string, string>)["Content-Type"] = "application/fhir+json";
          init.body =
            typeof cell.source.body === "string"
              ? interpolate(cell.source.body, scope)
              : JSON.stringify(deepInterpolate(cell.source.body, scope));
        }
        const res = await fetch(url, init);
        const data = await res.json();
        if (!res.ok) {
          return { kind: "error", severity: "error", message: `HTTP ${res.status}`, outcome: data };
        }
        const isBundleOrResource =
          data && typeof data === "object" && "resourceType" in (data as Record<string, unknown>);
        return isBundleOrResource ? { kind: "bundle", data } : { kind: "json", data };
      }

      case "pipeline": {
        const t0 = performance.now();
        const input = scope[cell.source.input];
        if (!input || typeof input !== "object" || !("rows" in input)) {
          return errOut(`Pipeline input "${cell.source.input || "?"}" has no table data.`);
        }
        const result = runPipeline(
          input as { columns: string[]; rows: unknown[][] },
          cell.source.steps,
          scope
        );
        return {
          kind: "table",
          columns: result.columns,
          rows: result.rows,
          meta: {
            rowCount: result.rows.length,
            executionTimeMs: Math.round(performance.now() - t0),
            truncated: false,
          },
        };
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
