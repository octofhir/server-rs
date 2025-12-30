import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

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

// Fetch all ViewDefinitions
async function fetchViewDefinitions(): Promise<ViewDefinition[]> {
  const res = await fetch("/fhir/ViewDefinition?_count=100");
  if (!res.ok) {
    throw new Error("Failed to fetch ViewDefinitions");
  }
  const bundle = await res.json();
  return bundle.entry?.map((e: { resource: ViewDefinition }) => e.resource) || [];
}

// Fetch a single ViewDefinition
async function fetchViewDefinition(id: string): Promise<ViewDefinition> {
  const res = await fetch(`/fhir/ViewDefinition/${id}`);
  if (!res.ok) {
    throw new Error("Failed to fetch ViewDefinition");
  }
  return res.json();
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
    const error = await res.json();
    throw new Error(error.issue?.[0]?.diagnostics || "Failed to run ViewDefinition");
  }

  const result = await res.json();

  // Parse the result
  const columns = result.parameter
    ?.find((p: { name: string }) => p.name === "columns")
    ?.part?.map((p: { name: string; valueString: string }) => ({
      name: p.name,
      type: p.valueString,
    })) || [];

  const rowCount = result.parameter?.find((p: { name: string }) => p.name === "rowCount")?.valueInteger || 0;

  const rowsData = result.parameter?.find((p: { name: string }) => p.name === "rows")?.resource?.data;
  const rows = rowsData ? JSON.parse(rowsData) : [];

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
    const error = await res.json();
    throw new Error(error.issue?.[0]?.diagnostics || "Failed to save ViewDefinition");
  }

  return res.json();
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
    const error = await res.json();
    throw new Error(error.issue?.[0]?.diagnostics || "Failed to generate SQL");
  }

  const result = await res.json();

  // Parse the result
  const sql = result.parameter?.find((p: { name: string }) => p.name === "sql")?.valueString || "";

  const columns = result.parameter
    ?.find((p: { name: string }) => p.name === "columns")
    ?.part?.map((p: { name: string; valueString: string }) => ({
      name: p.name,
      type: p.valueString,
    })) || [];

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
    queryFn: () => fetchViewDefinition(id!),
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
