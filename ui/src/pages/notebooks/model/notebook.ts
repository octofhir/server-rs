// OctoFHIR Notebook (.fhirnb) frontend model — see docs/ui-notebooks-spec.md §3.
// P1 subset: markdown, fhirpath, sql, sql-on-fhir, chart cells.

import type { ChartSpec } from "@octofhir/ui-kit";

export type { ChartSpec };

export type FhirMeta = { versionId?: string; lastUpdated?: string; profile?: string[] };

export type VarKind =
  | "string"
  | "integer"
  | "decimal"
  | "boolean"
  | "date"
  | "dateTime"
  | "code"
  | "json";

export interface Variable {
  name: string;
  kind: VarKind;
  value: unknown;
  widget?: "text" | "number" | "select" | "switch" | "date" | "none";
  options?: { label: string; value: unknown }[];
}

export type CellType =
  | "markdown"
  | "fhirpath"
  | "sql-on-fhir"
  | "sql"
  | "cql"
  | "graphql"
  | "rest"
  | "chart"
  | "pipeline"
  | "input";

export type OutputKind =
  | "table"
  | "value"
  | "json"
  | "bundle"
  | "chart"
  | "markdown"
  | "html"
  | "error";

export type Output =
  | {
      kind: "table";
      columns: string[];
      rows: unknown[][];
      meta: { rowCount: number; executionTimeMs?: number; truncated: boolean };
    }
  | {
      kind: "value";
      data: unknown[];
      meta?: { parseTime?: number; evalTime?: number; totalTime?: number };
    }
  | { kind: "json"; data: unknown }
  | { kind: "bundle"; data: unknown }
  | { kind: "chart"; spec: unknown }
  | { kind: "markdown"; text: string }
  | { kind: "html"; html: string }
  | { kind: "error"; severity: "error" | "warning"; message: string; outcome?: unknown };

export interface CellBase {
  id: string;
  name?: string;
  collapsed?: boolean;
  outputs?: Output[];
  execCount?: number;
}

export interface MarkdownCell extends CellBase {
  type: "markdown";
  source: string;
}
export interface FhirPathCell extends CellBase {
  type: "fhirpath";
  source: string;
  config?: { contextRef?: string; contextCell?: string };
}
export interface SqlOnFhirCell extends CellBase {
  type: "sql-on-fhir";
  source: Record<string, unknown>; // ViewDefinition
  config?: { limit?: number; editorMode?: "visual" | "json" };
}
export interface SqlCell extends CellBase {
  type: "sql";
  source: string;
  config?: { params?: unknown[] };
}
export interface ChartCell extends CellBase {
  type: "chart";
  source: { inputCell: string; spec: ChartSpec };
}
export interface CqlCell extends CellBase {
  type: "cql";
  source: string;
  config?: {
    context?: string;
    contextCell?: string;
    params?: Record<string, unknown>;
    validateOnly?: boolean;
  };
}
export interface GraphqlCell extends CellBase {
  type: "graphql";
  source: string;
  config?: { variables?: Record<string, unknown>; instance?: string };
}
export type HttpMethod = "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
export interface RestCell extends CellBase {
  type: "rest";
  source: { method: HttpMethod; url: string; headers?: Record<string, string>; body?: unknown };
}
export type AggFn = "count" | "sum" | "avg" | "min" | "max" | "first" | "last";
export interface Agg {
  fn: AggFn;
  col?: string;
  as: string;
}
export type Step =
  | { op: "filter"; where: string }
  | { op: "select"; columns: string[] }
  | { op: "rename"; map: Record<string, string> }
  | { op: "derive"; as: string; expr: string }
  | { op: "groupBy"; keys: string[]; agg: Agg[] }
  | { op: "sort"; by: string; dir?: "asc" | "desc" }
  | { op: "limit"; n: number }
  | { op: "distinct"; columns?: string[] };
export type StepOp = Step["op"];

export interface PipelineCell extends CellBase {
  type: "pipeline";
  source: { input: string; steps: Step[] };
  config?: { engine?: "auto" | "client" | "sql" };
}
export interface InputCell extends CellBase {
  type: "input";
  source: { variable: string };
  config?: { widget?: Variable["widget"]; options?: Variable["options"] };
}

export type Cell =
  | MarkdownCell
  | FhirPathCell
  | SqlOnFhirCell
  | SqlCell
  | ChartCell
  | CqlCell
  | GraphqlCell
  | RestCell
  | PipelineCell
  | InputCell;

export interface Notebook {
  resourceType: "Notebook";
  id?: string;
  meta?: FhirMeta;
  nbformat: 1;
  title: string;
  description?: string;
  fhirVersion?: "R4" | "R4B" | "R5" | "R6";
  owner?: string;
  tags?: string[];
  defaults?: { contextResourceType?: string; rowLimit?: number };
  variables?: Variable[];
  cells: Cell[];
}

/** Runtime scope: variable name / named-cell output → value. Not persisted. */
export type Scope = Record<string, unknown>;

export type CellStatus = "idle" | "running" | "stale" | "ok" | "error";

let counter = 0;
export function newCellId(): string {
  counter += 1;
  return `c${Date.now().toString(36)}${counter}`;
}

export function emptyNotebook(title = "Untitled notebook"): Notebook {
  return {
    resourceType: "Notebook",
    nbformat: 1,
    title,
    fhirVersion: "R4",
    cells: [
      {
        id: newCellId(),
        type: "markdown",
        source: `# ${title}\n\nStart writing. Add a query cell below.`,
      },
    ],
  };
}

/** Drop cached outputs + exec counters (clear-outputs-on-save). */
export function stripOutputs(nb: Notebook): Notebook {
  return {
    ...nb,
    cells: nb.cells.map(({ outputs: _o, execCount: _e, ...c }) => c as Cell),
  };
}

export function defaultCell(type: CellType): Cell {
  const id = newCellId();
  switch (type) {
    case "markdown":
      return { id, type, source: "Write **markdown** here." };
    case "fhirpath":
      return { id, type, source: "Patient.name.given" };
    case "sql":
      return {
        id,
        type,
        source: "SELECT id, resource->>'status' AS status\nFROM patient\nLIMIT 50",
      };
    case "sql-on-fhir":
      return {
        id,
        type,
        source: {
          resourceType: "ViewDefinition",
          resource: "Patient",
          select: [
            {
              column: [
                { name: "id", path: "id" },
                { name: "family", path: "name.family.first()" },
              ],
            },
          ],
        },
        config: { limit: 100, editorMode: "json" },
      };
    case "chart":
      return { id, type, source: { inputCell: "", spec: { type: "bar", series: [] } } };
    case "cql":
      return {
        id,
        type,
        source: "define InitialPopulation: [Patient] P where AgeInYears() > 40",
      };
    case "graphql":
      return { id, type, source: "{\n  PatientList(_count: 5) {\n    id\n  }\n}" };
    case "rest":
      return { id, type, source: { method: "GET", url: "/Patient?_count=10" } };
    case "pipeline":
      return { id, type, source: { input: "", steps: [] }, config: { engine: "client" } };
    case "input":
      return { id, type, source: { variable: "" }, config: { widget: "text" } };
    default:
      // P1 stub for not-yet-built cell types — render as markdown note.
      return { id, type: "markdown", source: `_${type} cell — coming soon_` };
  }
}
