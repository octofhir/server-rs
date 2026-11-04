import { fhirClient, HttpError } from "@/shared/api/fhirClient";
import { isRecord } from "@/shared/api/guards";

/**
 * The SQL on FHIR operations the console runs: `$sql-run` (SoF 3.0.0-ballot,
 * the rows of a ViewDefinition, SQLQuery or SQLView) and the server's
 * console-only `$sql-describe` (the columns and resolved dependencies of the
 * same subject, without reading data).
 */

export interface ResultColumn {
  name: string;
  type: string;
}

/** A provenance record of a terminology dependency, as the runner reports it. */
export interface TerminologyRecord {
  label?: string;
  resourceType: "ValueSet" | "ConceptMap";
  url: string;
  version?: string;
  /** Where the content came from: stored, package or the terminology server. */
  source?: string;
  expandedBy?: string;
  members?: number;
  mappings?: number;
}

export type DependencyKind = "ViewDefinition" | "SQLView" | "ValueSet" | "ConceptMap";

export interface DescribedDependency {
  label: string;
  canonical: string;
  kind: DependencyKind;
  /** The canonical with the version it resolved to. */
  resolved?: string;
  columns: ResultColumn[];
  provenance?: TerminologyRecord;
}

export interface Description {
  columns: ResultColumn[];
  dependencies: DescribedDependency[];
  /** Why the SQL does not prepare, when it does not. */
  check?: string;
}

export interface RunResult {
  columns: ResultColumn[];
  rowCount: number;
  rows: Record<string, unknown>[];
  /** Terminology the job resolved (the `x-sof-terminology` header). */
  terminology: TerminologyRecord[];
  elapsedMs: number;
}

/** The diagnostics of an OperationOutcome error, else the error's message. */
export function operationMessage(error: unknown): string {
  if (error instanceof HttpError) {
    const outcome = error.response.data;
    if (isRecord(outcome) && Array.isArray(outcome.issue)) {
      const messages = outcome.issue
        .filter(isRecord)
        .map((issue) => (typeof issue.diagnostics === "string" ? issue.diagnostics : undefined))
        .filter((m): m is string => !!m);
      if (messages.length > 0) return messages.join("\n");
    }
    if (typeof outcome === "string" && outcome.trim()) return outcome;
  }
  return error instanceof Error ? error.message : String(error);
}

function parts(value: unknown): Record<string, unknown>[] {
  return isRecord(value) && Array.isArray(value.part) ? value.part.filter(isRecord) : [];
}

function parameterList(value: unknown): Record<string, unknown>[] {
  return isRecord(value) && Array.isArray(value.parameter) ? value.parameter.filter(isRecord) : [];
}

function stringPart(list: Record<string, unknown>[], name: string): string | undefined {
  const part = list.find((p) => p.name === name);
  const value = part?.valueString ?? part?.valueCode;
  return typeof value === "string" ? value : undefined;
}

function readColumn(value: Record<string, unknown>): ResultColumn | undefined {
  const list = parts(value);
  const name = stringPart(list, "name");
  return name ? { name, type: stringPart(list, "type") ?? "" } : undefined;
}

function columnsOf(list: Record<string, unknown>[]): ResultColumn[] {
  return list
    .filter((p) => p.name === "column")
    .map(readColumn)
    .filter((c): c is ResultColumn => !!c);
}

function parseRecord(json: string | undefined): TerminologyRecord | undefined {
  if (!json) return undefined;
  try {
    const value: unknown = JSON.parse(json);
    return isRecord(value) && typeof value.url === "string" ? (value as unknown as TerminologyRecord) : undefined;
  } catch {
    return undefined;
  }
}

export function parseDescription(value: unknown): Description {
  const list = parameterList(value);
  const dependencies = list
    .filter((p) => p.name === "dependency")
    .map((p): DescribedDependency => {
      const dep = parts(p);
      return {
        label: stringPart(dep, "label") ?? "",
        canonical: stringPart(dep, "canonical") ?? "",
        kind: (stringPart(dep, "kind") ?? "ViewDefinition") as DependencyKind,
        resolved: stringPart(dep, "resolved"),
        columns: columnsOf(dep),
        provenance: parseRecord(stringPart(dep, "provenance")),
      };
    });
  return { columns: columnsOf(list), dependencies, check: stringPart(list, "check") };
}

function parseTerminologyHeader(header: string | undefined): TerminologyRecord[] {
  if (!header) return [];
  try {
    const value: unknown = JSON.parse(header);
    return Array.isArray(value) ? (value.filter(isRecord) as unknown as TerminologyRecord[]) : [];
  } catch {
    return [];
  }
}

function subjectParameters(subject: Record<string, unknown>) {
  return [{ name: "subjectResource", resource: subject }];
}

/** `$sql-describe` of `subject`. */
export async function sqlDescribe(subject: Record<string, unknown>): Promise<Description> {
  const response = await fhirClient.customRequest({
    method: "POST",
    url: "/fhir/$sql-describe",
    data: { resourceType: "Parameters", parameter: subjectParameters(subject) },
  });
  return parseDescription(response.data);
}

/**
 * `$sql-run` of `subject`, its rows as JSON. Columns come from `describe`
 * when given (types, and a header even for no rows), else from the rows.
 */
export async function sqlRun(
  subject: Record<string, unknown>,
  options: { limit: number; parameters?: Record<string, unknown>; describe?: Promise<Description | undefined> }
): Promise<RunResult> {
  const parameter: Record<string, unknown>[] = [
    ...subjectParameters(subject),
    { name: "_format", valueCode: "json" },
    { name: "_limit", valueInteger: options.limit },
  ];
  if (options.parameters) parameter.push({ name: "parameters", resource: options.parameters });

  const started = performance.now();
  const response = await fhirClient.customRequest({
    method: "POST",
    url: "/fhir/$sql-run",
    data: { resourceType: "Parameters", parameter },
    // The rows themselves, not a Binary envelope.
    headers: { Accept: "application/json" },
  });
  const elapsedMs = Math.round(performance.now() - started);
  const rows = Array.isArray(response.data) ? response.data.filter(isRecord) : [];
  const described = await options.describe?.catch(() => undefined);
  const columns =
    described && described.columns.length > 0
      ? described.columns
      : Object.keys(rows[0] ?? {}).map((name) => ({ name, type: "" }));
  return {
    columns,
    rowCount: rows.length,
    rows,
    terminology: parseTerminologyHeader(response.headers["x-sof-terminology"]),
    elapsedMs,
  };
}
