// Reactive dependency graph for a notebook. Builds the DAG from ${refs} in cell
// source/config plus explicit cell refs (chart.inputCell, config.contextCell), and
// derives scope, topological run order, and downstream-stale closures.
// See docs/ui-notebooks-plan.md §5.

import type { Cell, Notebook, Scope } from "./notebook";

const TOKEN = /\$\{([a-zA-Z_][\w.[\]]*)\}/g;

/** Root scope-names referenced via `${name}` / `${name.path}` in any string field. */
function refNames(cell: Cell): Set<string> {
  const names = new Set<string>();
  const scan = (v: unknown): void => {
    if (typeof v === "string") {
      for (const m of v.matchAll(TOKEN)) names.add(m[1].split(/[.[]/)[0]);
    } else if (Array.isArray(v)) {
      for (const x of v) scan(x);
    } else if (v && typeof v === "object") {
      for (const x of Object.values(v)) scan(x);
    }
  };
  scan(cell.source);
  if ("config" in cell && cell.config) scan(cell.config);
  // chart / pipeline bind to a named table cell by variable name — same as a ${ref}.
  if (cell.type === "chart" && cell.source.inputCell) names.add(cell.source.inputCell);
  if (cell.type === "pipeline" && cell.source.input) names.add(cell.source.input);
  return names;
}

/** Cell ids this cell points at explicitly (config.contextCell). */
function cellRefIds(cell: Cell): string[] {
  const ids: string[] = [];
  const cfg = ("config" in cell ? cell.config : undefined) as { contextCell?: string } | undefined;
  if (cfg?.contextCell) ids.push(cfg.contextCell);
  return ids;
}

/** Flat scope map: variable values + named-cell outputs. */
export function buildScope(nb: Notebook | null): Scope {
  const s: Scope = {};
  if (!nb) return s;
  for (const v of nb.variables ?? []) s[v.name] = v.value;
  for (const c of nb.cells) {
    if (!c.name || !c.outputs?.length) continue;
    const out = c.outputs[0];
    if (out.kind === "table") s[c.name] = { columns: out.columns, rows: out.rows };
    else if (out.kind === "value") s[c.name] = out.data;
    else if (out.kind === "json" || out.kind === "bundle") s[c.name] = out.data;
  }
  return s;
}

/** cellId → set of cell ids it depends on (edges point producer → consumer). */
export function depMap(nb: Notebook): Map<string, Set<string>> {
  const nameToCell = new Map<string, string>();
  for (const c of nb.cells) if (c.name) nameToCell.set(c.name, c.id);
  const deps = new Map<string, Set<string>>();
  for (const c of nb.cells) {
    const d = new Set<string>();
    for (const n of refNames(c)) {
      const pid = nameToCell.get(n);
      if (pid && pid !== c.id) d.add(pid);
    }
    for (const rid of cellRefIds(c)) if (rid !== c.id) d.add(rid);
    deps.set(c.id, d);
  }
  return deps;
}

/** Scope-names a cell consumes (var/cell refs resolved to producer names). */
function consumedNames(nb: Notebook, cell: Cell): Set<string> {
  const names = refNames(cell);
  const idToName = new Map(nb.cells.filter((c) => c.name).map((c) => [c.id, c.name as string]));
  for (const rid of cellRefIds(cell)) {
    const n = idToName.get(rid);
    if (n) names.add(n);
  }
  return names;
}

/**
 * Cell ids that become stale when the given scope-names change. Propagates through
 * named cells: a stale named cell's output changes → its consumers go stale too.
 */
export function staleClosure(nb: Notebook, changed: string[]): string[] {
  const consumers = new Map<string, string[]>();
  const idName = new Map(nb.cells.filter((c) => c.name).map((c) => [c.id, c.name as string]));
  for (const c of nb.cells) {
    for (const n of consumedNames(nb, c)) {
      const list = consumers.get(n);
      if (list) list.push(c.id);
      else consumers.set(n, [c.id]);
    }
  }
  const stale = new Set<string>();
  const queue = [...changed];
  while (queue.length) {
    const name = queue.shift() as string;
    for (const cid of consumers.get(name) ?? []) {
      if (stale.has(cid)) continue;
      stale.add(cid);
      const produced = idName.get(cid);
      if (produced) queue.push(produced);
    }
  }
  return [...stale];
}

/** Cells in dependency order (deps before dependents); falls back to array order. */
export function topoOrder(nb: Notebook): Cell[] {
  const deps = depMap(nb);
  const byId = new Map(nb.cells.map((c) => [c.id, c]));
  const visited = new Set<string>();
  const out: Cell[] = [];
  const visit = (id: string, stack: Set<string>): void => {
    if (visited.has(id) || stack.has(id)) return;
    stack.add(id);
    for (const d of deps.get(id) ?? []) if (byId.has(d)) visit(d, stack);
    stack.delete(id);
    visited.add(id);
    const cell = byId.get(id);
    if (cell) out.push(cell);
  };
  for (const c of nb.cells) visit(c.id, new Set());
  return out;
}
