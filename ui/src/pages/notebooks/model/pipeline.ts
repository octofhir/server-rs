// Client-side pipeline engine — runs a chain of transform steps over a cached
// table (a named cell's output). Small-data path; SQL-compile is deferred.
// See docs/ui-notebooks-spec.md §1.4.

import type { TabularData } from "@octofhir/ui-kit";
import { interpolate } from "./execution";
import type { Agg, Scope, Step } from "./notebook";

type Row = Record<string, unknown>;

function toRows(t: TabularData): Row[] {
  return t.rows.map((r) => {
    const o: Row = {};
    t.columns.forEach((c, i) => {
      o[c] = r[i];
    });
    return o;
  });
}

function toTable(rows: Row[], columns?: string[]): TabularData {
  const cols = columns ?? [...new Set(rows.flatMap((r) => Object.keys(r)))];
  return { columns: cols, rows: rows.map((r) => cols.map((c) => r[c] ?? null)) };
}

/** Compile a row expression (columns as identifiers) after ${var} interpolation. */
function compileExpr(expr: string, columns: string[], scope: Scope): (row: Row) => unknown {
  const src = interpolate(expr, scope);
  // new Function over user-owned data, client-only — no server trust boundary crossed.
  const fn = new Function(...columns, `"use strict"; return (${src});`) as (
    ...args: unknown[]
  ) => unknown;
  return (row: Row) => {
    try {
      return fn(...columns.map((c) => row[c]));
    } catch {
      return undefined;
    }
  };
}

function num(v: unknown): number {
  const n = typeof v === "number" ? v : Number(v);
  return Number.isFinite(n) ? n : 0;
}

function applyAgg(rows: Row[], agg: Agg): unknown {
  const vals = agg.col ? rows.map((r) => r[agg.col as string]) : [];
  switch (agg.fn) {
    case "count":
      return rows.length;
    case "sum":
      return vals.reduce((a: number, b) => a + num(b), 0);
    case "avg":
      return vals.length ? vals.reduce((a: number, b) => a + num(b), 0) / vals.length : 0;
    case "min":
      return vals.length ? Math.min(...vals.map(num)) : 0;
    case "max":
      return vals.length ? Math.max(...vals.map(num)) : 0;
    case "first":
      return vals[0] ?? null;
    case "last":
      return vals[vals.length - 1] ?? null;
    default:
      return null;
  }
}

function runStep(rows: Row[], step: Step, scope: Scope): Row[] {
  const cols = rows.length ? Object.keys(rows[0]) : [];
  switch (step.op) {
    case "filter": {
      const pred = compileExpr(step.where, cols, scope);
      return rows.filter((r) => Boolean(pred(r)));
    }
    case "select":
      return rows.map((r) => {
        const o: Row = {};
        for (const c of step.columns) o[c] = r[c];
        return o;
      });
    case "rename":
      return rows.map((r) => {
        const o: Row = {};
        for (const [k, v] of Object.entries(r)) o[step.map[k] ?? k] = v;
        return o;
      });
    case "derive": {
      const fn = compileExpr(step.expr, cols, scope);
      return rows.map((r) => ({ ...r, [step.as]: fn(r) ?? null }));
    }
    case "groupBy": {
      const groups = new Map<string, Row[]>();
      for (const r of rows) {
        const key = step.keys.map((k) => String(r[k])).join("");
        const g = groups.get(key);
        if (g) g.push(r);
        else groups.set(key, [r]);
      }
      return [...groups.values()].map((g) => {
        const o: Row = {};
        for (const k of step.keys) o[k] = g[0][k];
        for (const a of step.agg) o[a.as] = applyAgg(g, a);
        return o;
      });
    }
    case "sort": {
      const dir = step.dir === "desc" ? -1 : 1;
      return [...rows].sort((a, b) => {
        const av = a[step.by];
        const bv = b[step.by];
        if (av === bv) return 0;
        if (typeof av === "number" && typeof bv === "number") return (av - bv) * dir;
        return String(av) < String(bv) ? -dir : dir;
      });
    }
    case "limit":
      return rows.slice(0, Math.max(0, step.n));
    case "distinct": {
      const keyCols = step.columns?.length ? step.columns : rows.length ? Object.keys(rows[0]) : [];
      const seen = new Set<string>();
      const out: Row[] = [];
      for (const r of rows) {
        const key = keyCols.map((c) => JSON.stringify(r[c])).join("");
        if (seen.has(key)) continue;
        seen.add(key);
        out.push(r);
      }
      return out;
    }
    default:
      return rows;
  }
}

/** Run the full step chain client-side over the input table. */
export function runPipeline(input: TabularData, steps: Step[], scope: Scope): TabularData {
  let rows = toRows(input);
  for (const step of steps) rows = runStep(rows, step, scope);
  return toTable(rows);
}
