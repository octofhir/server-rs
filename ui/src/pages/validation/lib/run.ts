import { fhirClient, HttpError } from "@/shared/api/fhirClient";
import { serverApi } from "@/shared/api/serverApi";
import type { FhirOperationOutcome, FhirResource, PackageInfo } from "@/shared/api/types";

export interface ProfileHit {
  url: string;
  name: string;
  packageName: string;
  packageVersion: string;
}

/** Constraint StructureDefinitions on resources, searched across installed packages. */
export async function searchProfiles(packages: PackageInfo[], query: string): Promise<ProfileHit[]> {
  const results = await Promise.allSettled(
    packages.map(async (pkg) => {
      const response = await serverApi.getPackageResources(pkg.name, pkg.version, {
        resourceType: "StructureDefinition",
        search: query.replace(/\s+/g, "") || undefined,
        limit: 30,
      });
      return response.resources.flatMap((resource) =>
        resource.url && resource.derivation === "constraint" && resource.kind === "resource"
          ? [{ url: resource.url, name: resource.name ?? resource.url, packageName: pkg.name, packageVersion: pkg.version }]
          : []
      );
    })
  );
  return results
    .flatMap((result) => (result.status === "fulfilled" ? result.value : []))
    .sort((a, b) => a.name.localeCompare(b.name))
    .slice(0, 60);
}

/** The resource type a profile constrains (`StructureDefinition.type`). */
export async function profileType(hit: ProfileHit): Promise<string | undefined> {
  const definition = await serverApi.getPackageResourceContent(hit.packageName, hit.packageVersion, hit.url);
  const type = (definition as { type?: unknown } | undefined)?.type;
  return typeof type === "string" ? type : undefined;
}

export interface Issue {
  severity: "fatal" | "error" | "warning" | "information";
  message: string;
  expression?: string;
}

export interface ResourceResult {
  id: string;
  issues: Issue[];
}

export interface RunOptions {
  type: string;
  profile?: string;
  /** FHIR search parameters narrowing the sample, e.g. `_tag=demo-data`. */
  query: string;
  limit: number;
  signal: AbortSignal;
  onFetched: (count: number) => void;
  onResult: (result: ResourceResult) => void;
}

const PAGE = 100;
const CONCURRENCY = 6;

function searchParams(query: string): Record<string, string> {
  return Object.fromEntries(new URLSearchParams(query.replace(/^\?/, "")));
}

function outcomeIssues(outcome: FhirOperationOutcome | undefined): Issue[] {
  return (outcome?.issue ?? [])
    .filter((issue) => issue.severity !== "information")
    .map((issue) => ({
      severity: issue.severity as Issue["severity"],
      message: issue.diagnostics ?? issue.details?.text ?? issue.code ?? "Issue",
      expression: issue.expression?.[0],
    }));
}

async function validateOne(resource: FhirResource, profile: string | undefined): Promise<Issue[]> {
  const query = profile ? `?profile=${encodeURIComponent(profile)}` : "";
  try {
    const response = await fhirClient.rawRequest("POST", `/fhir/${resource.resourceType}/$validate${query}`, resource, {
      headers: { "Content-Type": "application/fhir+json", Accept: "application/fhir+json" },
    });
    return outcomeIssues(response.data as FhirOperationOutcome);
  } catch (error) {
    if (error instanceof HttpError) {
      const issues = outcomeIssues(error.response.data as FhirOperationOutcome);
      if (issues.length > 0) return issues;
    }
    return [{ severity: "fatal", message: error instanceof Error ? error.message : "Validation request failed" }];
  }
}

/**
 * Pages through the matching resources and validates them with a small worker pool, so
 * results stream in while later pages are still being read.
 */
export async function runValidation(options: RunOptions): Promise<void> {
  const queue: FhirResource[] = [];
  let fetched = 0;
  let done = false;

  const reader = (async () => {
    try {
      for (let offset = 0; fetched < options.limit && !options.signal.aborted; offset += PAGE) {
        const bundle = await fhirClient.search(options.type, {
          ...searchParams(options.query),
          _count: String(Math.min(PAGE, options.limit - fetched)),
          _offset: String(offset),
        });
        const resources = (bundle.entry ?? [])
          .filter((entry) => entry.search?.mode !== "outcome" && entry.resource?.resourceType === options.type)
          .flatMap((entry) => (entry.resource ? [entry.resource] : []));
        queue.push(...resources.slice(0, options.limit - fetched));
        fetched += Math.min(resources.length, options.limit - fetched);
        options.onFetched(fetched);
        if (resources.length < PAGE || !bundle.link?.some((link) => link.relation === "next")) break;
      }
    } finally {
      done = true;
    }
  })();

  const worker = async () => {
    while (!options.signal.aborted) {
      const resource = queue.shift();
      if (!resource) {
        if (done) return;
        await new Promise((resolve) => setTimeout(resolve, 20));
        continue;
      }
      const issues = await validateOne(resource, options.profile);
      if (!options.signal.aborted) options.onResult({ id: resource.id ?? "", issues });
    }
  };

  await Promise.all([reader, ...Array.from({ length: CONCURRENCY }, worker)]);
}

export interface IssueGroup {
  key: string;
  severity: Issue["severity"];
  message: string;
  expression?: string;
  resourceIds: string[];
}

/** Array indexes vary per resource; group `name[2].given` with `name[0].given`. */
function pathShape(expression: string | undefined): string | undefined {
  return expression?.replace(/\[\d+\]/g, "[n]");
}

const SEVERITY_ORDER: Record<Issue["severity"], number> = { fatal: 0, error: 1, warning: 2, information: 3 };

export function groupIssues(results: ResourceResult[]): IssueGroup[] {
  const groups = new Map<string, IssueGroup>();
  for (const result of results) {
    for (const issue of result.issues) {
      const expression = pathShape(issue.expression);
      const key = `${issue.severity}|${expression ?? ""}|${issue.message}`;
      const group = groups.get(key) ?? { key, severity: issue.severity, message: issue.message, expression, resourceIds: [] };
      if (!group.resourceIds.includes(result.id)) group.resourceIds.push(result.id);
      groups.set(key, group);
    }
  }
  return [...groups.values()].sort(
    (a, b) => SEVERITY_ORDER[a.severity] - SEVERITY_ORDER[b.severity] || b.resourceIds.length - a.resourceIds.length
  );
}
