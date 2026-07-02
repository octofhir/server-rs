// Effector store for the notebook editor — single source of truth for the document,
// per-cell run status, and the derived reactive scope. Replaces component useState.
// See docs/ui-notebooks-plan.md §0 (task list) + §8c.

import { combine, createEvent, createStore, sample } from "effector";
import { buildScope, staleClosure, topoOrder } from "./dag";
import { runCell } from "./execution";
import {
  type Cell,
  type CellStatus,
  type CellType,
  defaultCell,
  type Notebook,
  newCellId,
  type Output,
  type Scope,
  type Variable,
} from "./notebook";

/** Cell types that execute against an engine (everything but prose/inputs/charts). */
export function isRunnable(cell: Cell): boolean {
  return cell.type !== "markdown" && cell.type !== "input" && cell.type !== "chart";
}

// ── events ─────────────────────────────────────────────────────────────────
export const notebookLoaded = createEvent<Notebook>();
export const titleChanged = createEvent<string>();
export const cellChanged = createEvent<{ id: string; next: Cell }>();
export const cellTypeChanged = createEvent<{ id: string; type: CellType }>();
export const cellAdded = createEvent<CellType>();
export const cellMoved = createEvent<{ id: string; dir: -1 | 1 }>();
export const cellDuplicated = createEvent<string>();
export const cellDeleted = createEvent<string>();
export const cellCollapseToggled = createEvent<string>();

export const variableAdded = createEvent<Variable>();
export const variableUpdated = createEvent<{ index: number; next: Variable }>();
export const variableRemoved = createEvent<number>();
export const variableValueSet = createEvent<{ name: string; value: unknown }>();

const cellRunStarted = createEvent<string>();
const cellRan = createEvent<{ id: string; output: Output }>();
const markStale = createEvent<string[]>();

// ── stores ─────────────────────────────────────────────────────────────────
export const $notebook = createStore<Notebook | null>(null);
export const $statuses = createStore<Record<string, CellStatus>>({});
export const $scope = $notebook.map<Scope>(buildScope);

export const $namedCells = $notebook.map((nb) =>
  (nb?.cells ?? [])
    .filter((c) => c.name && c.outputs?.some((o) => o.kind === "table"))
    .map((c) => ({ id: c.id, name: c.name as string, label: `${c.name} (${c.type})` }))
);

export const $variables = $notebook.map((nb) => nb?.variables ?? []);

// ── notebook reducers ────────────────────────────────────────────────────────
const mapCells = (nb: Notebook | null, fn: (cells: Cell[]) => Cell[]): Notebook | null =>
  nb ? { ...nb, cells: fn(nb.cells) } : nb;

$notebook
  .on(notebookLoaded, (_s, nb) => nb)
  .on(titleChanged, (nb, title) => (nb ? { ...nb, title } : nb))
  .on(cellChanged, (nb, { id, next }) =>
    mapCells(nb, (cells) => cells.map((c) => (c.id === id ? next : c)))
  )
  .on(cellTypeChanged, (nb, { id, type }) =>
    mapCells(nb, (cells) =>
      cells.map((c) => {
        if (c.id !== id) return c;
        const fresh = defaultCell(type);
        return { ...fresh, id: c.id, name: c.name } as Cell;
      })
    )
  )
  .on(cellAdded, (nb, type) => mapCells(nb, (cells) => [...cells, defaultCell(type)]))
  .on(cellMoved, (nb, { id, dir }) =>
    mapCells(nb, (cells) => {
      const i = cells.findIndex((c) => c.id === id);
      const j = i + dir;
      if (i < 0 || j < 0 || j >= cells.length) return cells;
      const next = [...cells];
      [next[i], next[j]] = [next[j], next[i]];
      return next;
    })
  )
  .on(cellDuplicated, (nb, id) =>
    mapCells(nb, (cells) => {
      const i = cells.findIndex((c) => c.id === id);
      if (i < 0) return cells;
      const copy: Cell = {
        ...cells[i],
        id: newCellId(),
        name: undefined,
        outputs: undefined,
        execCount: undefined,
      };
      const next = [...cells];
      next.splice(i + 1, 0, copy);
      return next;
    })
  )
  .on(cellDeleted, (nb, id) => mapCells(nb, (cells) => cells.filter((c) => c.id !== id)))
  .on(cellCollapseToggled, (nb, id) =>
    mapCells(nb, (cells) =>
      cells.map((c) => (c.id === id ? { ...c, collapsed: !(c.collapsed ?? false) } : c))
    )
  )
  .on(cellRan, (nb, { id, output }) =>
    mapCells(nb, (cells) =>
      cells.map((c) =>
        c.id === id ? { ...c, outputs: [output], execCount: (c.execCount ?? 0) + 1 } : c
      )
    )
  )
  .on(variableAdded, (nb, v) => (nb ? { ...nb, variables: [...(nb.variables ?? []), v] } : nb))
  .on(variableUpdated, (nb, { index, next }) =>
    nb ? { ...nb, variables: (nb.variables ?? []).map((v, i) => (i === index ? next : v)) } : nb
  )
  .on(variableRemoved, (nb, index) =>
    nb ? { ...nb, variables: (nb.variables ?? []).filter((_v, i) => i !== index) } : nb
  )
  .on(variableValueSet, (nb, { name, value }) =>
    nb
      ? {
          ...nb,
          variables: (nb.variables ?? []).map((v) => (v.name === name ? { ...v, value } : v)),
        }
      : nb
  );

// ── status reducers ──────────────────────────────────────────────────────────
$statuses
  .reset(notebookLoaded)
  .on(cellRunStarted, (s, id) => ({ ...s, [id]: "running" as CellStatus }))
  .on(cellRan, (s, { id, output }) => ({
    ...s,
    [id]: (output.kind === "error" ? "error" : "ok") as CellStatus,
  }))
  .on(cellChanged, (s, { id }) => (s[id] === "ok" ? { ...s, [id]: "stale" } : s))
  .on(cellTypeChanged, (s, { id }) => ({ ...s, [id]: "idle" as CellStatus }))
  .on(cellDeleted, (s, id) => {
    const { [id]: _drop, ...rest } = s;
    return rest;
  })
  .on(markStale, (s, ids) => {
    if (!ids.length) return s;
    const next = { ...s };
    for (const id of ids) next[id] = "stale";
    return next;
  });

// Editing a named cell's output (on run) or a variable value marks the downstream
// dependency closure stale (in dependency order via the DAG).
sample({
  clock: cellRan,
  source: $notebook,
  fn: (nb, { id }) => {
    if (!nb) return [];
    const cell = nb.cells.find((c) => c.id === id);
    if (!cell?.name) return [];
    return staleClosure(nb, [cell.name]).filter((x) => x !== id);
  },
  target: markStale,
});

sample({
  clock: variableValueSet,
  source: $notebook,
  fn: (nb, { name }) => (nb ? staleClosure(nb, [name]) : []),
  target: markStale,
});

// ── run orchestration (plain async — events are synchronous, so $scope is fresh
//    between sequential runs) ──────────────────────────────────────────────────
export async function runOne(cell: Cell): Promise<void> {
  cellRunStarted(cell.id);
  const output = await runCell(cell, $scope.getState());
  cellRan({ id: cell.id, output });
}

async function runSequence(cells: Cell[]): Promise<void> {
  for (const c of cells) {
    if (isRunnable(c)) await runOne(c);
  }
}

export function runAll(): Promise<void> {
  const nb = $notebook.getState();
  return nb ? runSequence(topoOrder(nb)) : Promise.resolve();
}

/** Run the given cell and every cell positioned after it, in dependency order. */
export function runBelow(id: string): Promise<void> {
  const nb = $notebook.getState();
  if (!nb) return Promise.resolve();
  const from = nb.cells.findIndex((c) => c.id === id);
  if (from < 0) return Promise.resolve();
  const below = new Set(nb.cells.slice(from).map((c) => c.id));
  return runSequence(topoOrder(nb).filter((c) => below.has(c.id)));
}

/** Run only cells currently marked stale, in dependency order. */
export function runStale(): Promise<void> {
  const nb = $notebook.getState();
  if (!nb) return Promise.resolve();
  const st = $statuses.getState();
  return runSequence(topoOrder(nb).filter((c) => st[c.id] === "stale"));
}

export const $editorState = combine({
  notebook: $notebook,
  statuses: $statuses,
  scope: $scope,
  namedCells: $namedCells,
});
