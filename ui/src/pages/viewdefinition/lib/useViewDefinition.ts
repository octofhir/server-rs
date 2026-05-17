import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { isRecord } from "@/shared/api/guards";

export interface ViewDefinitionColumn {
  name: string;
  path: string;
  type?: string;
  collection?: boolean;
  description?: string;
  _id?: string; // Internal ID for React key and drag-and-drop
}

export interface ViewDefinitionSelect {
  forEach?: string;
  forEachOrNull?: string;
  column?: ViewDefinitionColumn[];
  select?: ViewDefinitionSelect[];
  _id?: string; // Internal ID for React key
}

export interface ViewDefinitionWhere {
  path: string;
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
  status: "draft" | "active" | "retired" | "unknown";
  resource: string;
  description?: string;
  select: ViewDefinitionSelect[];
  where?: ViewDefinitionWhere[];
  constant?: ViewDefinitionConstant[];
}

export interface RunResult {
  columns: Array<{ name: string; type: string }>;
  rowCount: number;
  rows: unknown[];
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

function readDiagnostics(value: unknown): string | undefined {
  if (!isRecord(value) || !Array.isArray(value.issue)) {
    return undefined;
  }

  const issue = value.issue.find(isRecord);
  return typeof issue?.diagnostics === "string" ? issue.diagnostics : undefined;
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

function readRows(parameter: Record<string, unknown> | undefined): unknown[] {
  const resource = parameter && isRecord(parameter.resource) ? parameter.resource : undefined;
  const rowsData = resource?.data;
  if (typeof rowsData !== "string") {
    return [];
  }

  try {
    const rows = JSON.parse(rowsData);
    return Array.isArray(rows) ? rows : [];
  } catch {
    return [];
  }
}

// Fetch all ViewDefinitions
async function fetchViewDefinitions(): Promise<ViewDefinition[]> {
  const res = await fetch("/fhir/ViewDefinition?_count=100");
  if (!res.ok) {
    throw new Error("Failed to fetch ViewDefinitions");
  }
  const bundle: unknown = await res.json();
  if (!isRecord(bundle) || !Array.isArray(bundle.entry)) {
    return [];
  }
  return bundle.entry
    .filter(isRecord)
    .map((entry) => entry.resource)
    .filter(isViewDefinition);
}

// Fetch a single ViewDefinition
async function fetchViewDefinition(id: string): Promise<ViewDefinition> {
  const res = await fetch(`/fhir/ViewDefinition/${id}`);
  if (!res.ok) {
    throw new Error("Failed to fetch ViewDefinition");
  }
  const viewDefinition: unknown = await res.json();
  if (!isViewDefinition(viewDefinition)) {
    throw new Error("Invalid ViewDefinition response");
  }
  return viewDefinition;
}

// Run a ViewDefinition
async function runViewDefinition(viewDefinition: ViewDefinition): Promise<RunResult> {
  const params = {
    resourceType: "Parameters",
    parameter: [
      { name: "viewDefinition", resource: viewDefinition },
      { name: "limit", valueInteger: 100 },
    ],
  };

  const res = await fetch("/fhir/ViewDefinition/$run", {
    method: "POST",
    headers: { "Content-Type": "application/fhir+json" },
    body: JSON.stringify(params),
  });

  if (!res.ok) {
    const error: unknown = await res.json();
    throw new Error(readDiagnostics(error) || "Failed to run ViewDefinition");
  }

  const result: unknown = await res.json();

  // Parse the result
  const columns = readColumns(findParameter(result, "columns"));
  const rowCountValue = findParameter(result, "rowCount")?.valueInteger;
  const rowCount = typeof rowCountValue === "number" ? rowCountValue : 0;
  const rows = readRows(findParameter(result, "rows"));

  return { columns, rowCount, rows };
}

// Save a ViewDefinition
async function saveViewDefinition(viewDefinition: ViewDefinition): Promise<ViewDefinition> {
  const method = viewDefinition.id ? "PUT" : "POST";
  const url = viewDefinition.id
    ? `/fhir/ViewDefinition/${viewDefinition.id}`
    : "/fhir/ViewDefinition";

  const res = await fetch(url, {
    method,
    headers: { "Content-Type": "application/fhir+json" },
    body: JSON.stringify(viewDefinition),
  });

  if (!res.ok) {
    const error: unknown = await res.json();
    throw new Error(readDiagnostics(error) || "Failed to save ViewDefinition");
  }

  const saved: unknown = await res.json();
  if (!isViewDefinition(saved)) {
    throw new Error("Invalid ViewDefinition save response");
  }
  return saved;
}

// Delete a ViewDefinition
async function deleteViewDefinition(id: string): Promise<void> {
  const res = await fetch(`/fhir/ViewDefinition/${id}`, { method: "DELETE" });
  if (!res.ok) {
    throw new Error("Failed to delete ViewDefinition");
  }
}

// Generate SQL from a ViewDefinition
async function generateSql(viewDefinition: ViewDefinition): Promise<SqlResult> {
  const params = {
    resourceType: "Parameters",
    parameter: [{ name: "viewDefinition", resource: viewDefinition }],
  };

  const res = await fetch("/fhir/ViewDefinition/$sql", {
    method: "POST",
    headers: { "Content-Type": "application/fhir+json" },
    body: JSON.stringify(params),
  });

  if (!res.ok) {
    const error: unknown = await res.json();
    throw new Error(readDiagnostics(error) || "Failed to generate SQL");
  }

  const result: unknown = await res.json();

  // Parse the result
  const sqlValue = findParameter(result, "sql")?.valueString;
  const sql = typeof sqlValue === "string" ? sqlValue : "";
  const columns = readColumns(findParameter(result, "columns"));

  return { sql, columns };
}

// Hooks
export function useViewDefinitions() {
  return useQuery({
    queryKey: ["viewDefinitions"],
    queryFn: fetchViewDefinitions,
  });
}

export function useViewDefinition(id: string | undefined) {
  return useQuery({
    queryKey: ["viewDefinition", id],
    queryFn: () => {
      if (!id) {
        throw new Error("ViewDefinition id is required");
      }
      return fetchViewDefinition(id);
    },
    enabled: !!id,
  });
}

export function useRunViewDefinition() {
  return useMutation({
    mutationFn: runViewDefinition,
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
    mutationFn: deleteViewDefinition,
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
