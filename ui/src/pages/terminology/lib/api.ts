import { fhirClient, HttpError } from "@/shared/api/fhirClient";
import { serverApi } from "@/shared/api/serverApi";
import type { FhirOperationOutcome, PackageInfo } from "@/shared/api/types";

export type TerminologyKind = "ValueSet" | "CodeSystem" | "ConceptMap";

export interface CanonicalHit {
  kind: TerminologyKind;
  url: string;
  name: string;
  version?: string;
  packageName: string;
  packageVersion: string;
}

/** Hits per package; the merged list is cut to {@link SEARCH_LIMIT}. */
const PER_PACKAGE = 40;
export const SEARCH_LIMIT = 100;

/**
 * Searches every installed package for terminology resources. The server has no
 * cross-package canonical search, so packages are queried in parallel and merged.
 */
export async function searchCanonicals(
  packages: PackageInfo[],
  kind: TerminologyKind,
  query: string
): Promise<{ hits: CanonicalHit[]; total: number }> {
  const results = await Promise.allSettled(
    packages.map(async (pkg) => {
      const response = await serverApi.getPackageResources(pkg.name, pkg.version, {
        resourceType: kind,
        // The server matches one substring; canonical names are CamelCase, so
        // "administrative gender" is sent as "administrativegender".
        search: query.replace(/\s+/g, "") || undefined,
        limit: PER_PACKAGE,
      });
      return {
        total: response.total,
        hits: response.resources.flatMap((resource) =>
          resource.url
            ? [
                {
                  kind,
                  url: resource.url,
                  name: resource.name ?? resource.id ?? resource.url,
                  version: resource.version,
                  packageName: pkg.name,
                  packageVersion: pkg.version,
                },
              ]
            : []
        ),
      };
    })
  );
  const fulfilled = results.flatMap((result) => (result.status === "fulfilled" ? [result.value] : []));
  const hits = fulfilled
    .flatMap((result) => result.hits)
    .sort((a, b) => a.name.localeCompare(b.name))
    .slice(0, SEARCH_LIMIT);
  return { hits, total: fulfilled.reduce((sum, result) => sum + result.total, 0) };
}

export async function fetchCanonical(hit: CanonicalHit): Promise<Record<string, unknown>> {
  const data = await serverApi.getPackageResourceContent(hit.packageName, hit.packageVersion, hit.url);
  return data && typeof data === "object" ? (data as Record<string, unknown>) : {};
}

/** A failed terminology call, with the server's OperationOutcome when there is one. */
export class TerminologyError extends Error {
  outcome?: FhirOperationOutcome;

  constructor(message: string, outcome?: FhirOperationOutcome) {
    super(message);
    this.name = "TerminologyError";
    this.outcome = outcome;
  }
}

type ParameterValue =
  | { valueUri: string }
  | { valueCode: string }
  | { valueString: string }
  | { valueInteger: number }
  | { valueBoolean: boolean };

export interface Parameter {
  name: string;
  valueString?: string;
  valueCode?: string;
  valueUri?: string;
  valueBoolean?: boolean;
  valueInteger?: number;
  valueCoding?: { system?: string; code?: string; display?: string };
  part?: Parameter[];
}

export interface Parameters {
  resourceType: "Parameters";
  parameter?: Parameter[];
}

/**
 * Terminology operations are POSTed as Parameters: the server ignores query-string
 * parameters on these operations.
 */
async function operation<T>(path: string, parameters: Record<string, ParameterValue | undefined>): Promise<T> {
  const body = {
    resourceType: "Parameters",
    parameter: Object.entries(parameters).flatMap(([name, value]) => (value ? [{ name, ...value }] : [])),
  };
  try {
    const response = await fhirClient.rawRequest("POST", `/fhir/${path}`, body, {
      headers: { "Content-Type": "application/fhir+json", Accept: "application/fhir+json" },
    });
    return response.data as T;
  } catch (error) {
    if (error instanceof HttpError) {
      const outcome = error.response.data as FhirOperationOutcome | undefined;
      const message = outcome?.issue?.[0]?.diagnostics ?? error.message;
      throw new TerminologyError(message, outcome?.resourceType === "OperationOutcome" ? outcome : undefined);
    }
    throw error;
  }
}

export interface ExpansionConcept {
  system?: string;
  code?: string;
  display?: string;
  abstract?: boolean;
  inactive?: boolean;
}

export interface Expansion {
  total?: number;
  offset?: number;
  contains: ExpansionConcept[];
}

export async function expandValueSet(url: string, filter: string, count: number, offset = 0): Promise<Expansion> {
  const result = await operation<{ expansion?: { total?: number; offset?: number; contains?: ExpansionConcept[] } }>(
    "ValueSet/$expand",
    {
      url: { valueUri: url },
      filter: filter.trim() ? { valueString: filter.trim() } : undefined,
      count: { valueInteger: count },
      offset: offset ? { valueInteger: offset } : undefined,
    }
  );
  const expansion = result.expansion ?? {};
  return { total: expansion.total, offset: expansion.offset, contains: expansion.contains ?? [] };
}

export function validateCode(url: string, system: string, code: string): Promise<Parameters> {
  return operation("ValueSet/$validate-code", {
    url: { valueUri: url },
    system: system.trim() ? { valueUri: system.trim() } : undefined,
    code: { valueCode: code.trim() },
    _explain: { valueBoolean: true },
  });
}

/** What `_explain` reports about a definition, as one line. */
function explainedDefinition(parts: Parameter[]) {
  const value = (name: string) => {
    const part = parts.find((item) => item.name === name);
    return part?.valueUri ?? part?.valueString ?? part?.valueCode;
  };
  const version = value("version");
  return `${value("url") ?? value("type")}${version ? `|${version}` : ""} · ${value("source")}`;
}

/** Which definitions decided a `$validate-code` result. */
export function explainLines(result: Parameters): string[] {
  const explain = result.parameter?.find((parameter) => parameter.name === "explain");
  const parts = explain?.part ?? [];
  if (parts.length === 0) return [];
  const lines = [explainedDefinition(parts)];
  const method = parts.find((part) => part.name === "method")?.valueCode;
  if (method) lines[0] += ` · by ${method}`;
  const codeSystem = parts.find((part) => part.name === "codeSystem");
  if (codeSystem?.part) lines.push(explainedDefinition(codeSystem.part));
  return lines;
}

export function lookupCode(system: string, code: string): Promise<Parameters> {
  return operation("CodeSystem/$lookup", { system: { valueUri: system }, code: { valueCode: code } });
}

export function subsumes(system: string, codeA: string, codeB: string): Promise<Parameters> {
  return operation("CodeSystem/$subsumes", {
    system: { valueUri: system },
    codeA: { valueCode: codeA.trim() },
    codeB: { valueCode: codeB.trim() },
  });
}

export function translate(url: string, system: string, code: string): Promise<Parameters> {
  return operation("ConceptMap/$translate", {
    url: { valueUri: url },
    system: system.trim() ? { valueUri: system.trim() } : undefined,
    code: { valueCode: code.trim() },
  });
}

/** First value of a named output parameter. */
export function parameterValue(parameters: Parameters | undefined, name: string): string | boolean | number | undefined {
  const parameter = parameters?.parameter?.find((entry) => entry.name === name);
  if (!parameter) return undefined;
  return (
    parameter.valueString ??
    parameter.valueCode ??
    parameter.valueUri ??
    parameter.valueBoolean ??
    parameter.valueInteger ??
    parameter.valueCoding?.code
  );
}
