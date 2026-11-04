import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { fhirClient } from "@/shared/api/fhirClient";
import { isRecord } from "@/shared/api/guards";
import { type RunResult, sqlDescribe, sqlRun } from "./sofApi";
import {
  LIBRARY_TYPES,
  libraryFromQuery,
  queryFromLibrary,
  runParameters,
  type SqlQuery,
  sqlLibraryKind,
} from "./sqlQuery";

export type { RunResult } from "./sofApi";

export interface ViewDefinitionColumnTag {
  name: string;
  value?: string;
}

export interface ViewDefinitionColumn {
  name: string;
  path: string;
  type?: string;
  collection?: boolean;
  description?: string;
  tag?: ViewDefinitionColumnTag[];
  _id?: string; // Internal ID for React key and drag-and-drop
}

/**
 * A `select` node in the SQL-on-FHIR ViewDefinition tree. Fully recursive:
 * a node may carry its own `column[]`, nested `select[]`, an iteration
 * (`forEach` / `forEachOrNull`), and `unionAll[]` branches.
 */
export interface ViewDefinitionSelect {
  forEach?: string;
  forEachOrNull?: string;
  column?: ViewDefinitionColumn[];
  select?: ViewDefinitionSelect[];
  unionAll?: ViewDefinitionSelect[];
  _id?: string; // Internal ID for React key
}

export interface ViewDefinitionWhere {
  path: string;
  description?: string;
  _id?: string; // Internal ID for React key
}

export interface ViewDefinitionConstant {
  name: string;
  valueString?: string;
  valueInteger?: number;
  valueBoolean?: boolean;
  valueDecimal?: number;
  _id?: string; // Internal ID for React key
}

export interface ViewDefinition {
  resourceType: "ViewDefinition";
  id?: string;
  url?: string;
  name: string;
  title?: string;
  status: "draft" | "active" | "retired" | "unknown";
  resource: string;
  fhirVersion?: string[];
  profile?: string[];
  description?: string;
  select: ViewDefinitionSelect[];
  where?: ViewDefinitionWhere[];
  constant?: ViewDefinitionConstant[];
}

export interface SqlResult {
  sql: string;
  columns: Array<{ name: string; type: string }>;
}

function isViewDefinition(value: unknown): value is ViewDefinition {
  return (
    isRecord(value) &&
    value.resourceType === "ViewDefinition" &&
    typeof value.name === "string" &&
    typeof value.status === "string" &&
    typeof value.resource === "string" &&
    Array.isArray(value.select)
  );
}

function readParameters(value: unknown): Array<Record<string, unknown>> {
  if (!isRecord(value) || !Array.isArray(value.parameter)) {
    return [];
  }
  return value.parameter.filter(isRecord);
}

function findParameter(value: unknown, name: string): Record<string, unknown> | undefined {
  return readParameters(value).find((parameter) => parameter.name === name);
}

function readColumns(parameter: Record<string, unknown> | undefined): SqlResult["columns"] {
  if (!parameter || !Array.isArray(parameter.part)) {
    return [];
  }

  return parameter.part.filter(isRecord).flatMap((part) => {
    if (typeof part.name !== "string" || typeof part.valueString !== "string") {
      return [];
    }
    return [{ name: part.name, type: part.valueString }];
  });
}

/**
 * Strip editor-internal `_id` keys (used only for stable React keys / drag
 * reordering) before sending a ViewDefinition to the server.
 */
function stripInternal<T>(value: T): T {
  if (Array.isArray(value)) {
    return value.map(stripInternal) as unknown as T;
  }
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [key, val] of Object.entries(value as Record<string, unknown>)) {
      if (key === "_id") continue;
      out[key] = stripInternal(val);
    }
    return out as T;
  }
  return value;
}

const LIBRARY_TYPE_SEARCH = `${LIBRARY_TYPES}|sql-query,${LIBRARY_TYPES}|sql-view`;

/** Saved ViewDefinitions. */
async function fetchViewDefinitions(): Promise<ViewDefinition[]> {
  const bundle = await fhirClient.search("ViewDefinition", { _count: 200 });
  return (bundle.entry ?? [])
    .map((entry) => entry.resource as unknown)
    .filter(isViewDefinition)
    .sort(byName);
}

/** Saved SQLQuery and SQLView Libraries. */
async function fetchSqlQueries(): Promise<SqlQuery[]> {
  const bundle = await fhirClient.search("Library", {
    type: LIBRARY_TYPE_SEARCH,
    _count: 200,
  });
  return (bundle.entry ?? [])
    .map((entry) => entry.resource as unknown)
    .filter((resource): resource is Record<string, unknown> => !!sqlLibraryKind(resource))
    .map(queryFromLibrary)
    .sort(byName);
}

const byName = (a: { name: string }, b: { name: string }) =>
  (a.name || "").localeCompare(b.name || "");

/** Rows of a ViewDefinition through `$sql-run`, typed by `$sql-describe`. */
export function runViewDefinition(viewDefinition: ViewDefinition, limit = 1000): Promise<RunResult> {
  const subject = stripInternal(viewDefinition) as unknown as Record<string, unknown>;
  return sqlRun(subject, { limit, describe: sqlDescribe(subject) });
}

/** Rows of a SQLQuery / SQLView through `$sql-run`. */
export function runSqlQuery(query: SqlQuery, limit = 1000): Promise<RunResult> {
  const subject = libraryFromQuery(query);
  return sqlRun(subject, {
    limit,
    parameters: query.kind === "sql-query" ? runParameters(query) : undefined,
    describe: sqlDescribe(subject),
  });
}

async function saveViewDefinition(viewDefinition: ViewDefinition): Promise<ViewDefinition> {
  const resource = stripInternal(viewDefinition);
  const saved: unknown = resource.id
    ? await fhirClient.update(resource as never)
    : await fhirClient.create(resource as never);
  if (!isViewDefinition(saved)) {
    throw new Error("Invalid ViewDefinition save response");
  }
  return saved;
}

async function saveSqlQuery(query: SqlQuery): Promise<SqlQuery> {
  const library = libraryFromQuery(query);
  const saved: unknown = query.id
    ? await fhirClient.update(library as never)
    : await fhirClient.create(library as never);
  if (!isRecord(saved) || !sqlLibraryKind(saved)) {
    throw new Error("Invalid Library save response");
  }
  const next = queryFromLibrary(saved);
  // Run-time values are console state, not part of the Library.
  next.parameters = next.parameters.map((p) => ({
    ...p,
    value: query.parameters.find((q) => q.name === p.name)?.value,
  }));
  return next;
}

/** Generated PostgreSQL of a ViewDefinition (informational). */
async function generateSql(viewDefinition: ViewDefinition): Promise<SqlResult> {
  const response = await fhirClient.customRequest({
    method: "POST",
    url: "/fhir/ViewDefinition/$sql",
    data: {
      resourceType: "Parameters",
      parameter: [{ name: "viewDefinition", resource: stripInternal(viewDefinition) }],
    },
  });
  const sqlValue = findParameter(response.data, "sql")?.valueString;
  return {
    sql: typeof sqlValue === "string" ? sqlValue : "",
    columns: readColumns(findParameter(response.data, "columns")),
  };
}

// Hooks
export function useViewDefinitions() {
  return useQuery({
    queryKey: ["viewDefinitions"],
    queryFn: fetchViewDefinitions,
  });
}

export function useSqlQueries() {
  return useQuery({
    queryKey: ["sqlQueries"],
    queryFn: fetchSqlQueries,
  });
}

export function useSaveSqlQuery() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: saveSqlQuery,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["sqlQueries"] }),
  });
}

export function useDeleteSqlQuery() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => fhirClient.delete("Library", id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["sqlQueries"] }),
  });
}

export function useSaveViewDefinition() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: saveViewDefinition,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["viewDefinitions"] });
    },
  });
}

export function useDeleteViewDefinition() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => fhirClient.delete("ViewDefinition", id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["viewDefinitions"] });
    },
  });
}

export function useGenerateSql() {
  return useMutation({
    mutationFn: generateSql,
  });
}
