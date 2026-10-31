import type { FhirBundle, FhirResource } from "@/shared/api/types";

export type VersionAction = "created" | "updated" | "deleted";

export interface ResourceVersion {
  versionId: string;
  /** ISO timestamp of the change. */
  at: string | undefined;
  action: VersionAction;
  /** Absent for a delete. */
  resource: FhirResource | undefined;
}

function actionOf(method: string | undefined, resource: FhirResource | undefined): VersionAction {
  if (method === "DELETE" || !resource) return "deleted";
  return method === "POST" ? "created" : "updated";
}

/** History bundle entries as versions, newest first. */
export function toVersions(bundle: FhirBundle | undefined): ResourceVersion[] {
  return (bundle?.entry ?? []).flatMap((entry, index) => {
    const resource = entry.resource;
    const etag = entry.response?.etag?.match(/W\/"(.+)"/)?.[1];
    const versionId = resource?.meta?.versionId ?? etag ?? `#${index}`;
    return [
      {
        versionId,
        at: resource?.meta?.lastUpdated ?? entry.response?.lastModified,
        action: actionOf(entry.request?.method, resource),
        resource,
      },
    ];
  });
}

/** The resource without the fields every version changes, so a diff shows only real edits. */
export function comparable(resource: FhirResource | undefined): unknown {
  if (!resource) return undefined;
  const { meta, ...rest } = resource;
  if (!meta) return rest;
  const { versionId: _version, lastUpdated: _updated, ...keptMeta } = meta;
  return Object.keys(keptMeta).length > 0 ? { ...rest, meta: keptMeta } : rest;
}

export function toDiffText(resource: FhirResource | undefined): string {
  const value = comparable(resource);
  return value === undefined ? "" : JSON.stringify(value, null, 2);
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Paths that differ between two JSON values, e.g. `telecom[0].value`, shallowest first.
 * A whole added or removed subtree is reported once at its root.
 */
export function changedPaths(before: unknown, after: unknown, path = ""): string[] {
  if (Object.is(before, after)) return [];
  if (Array.isArray(before) && Array.isArray(after)) {
    const paths: string[] = [];
    for (let index = 0; index < Math.max(before.length, after.length); index++) {
      paths.push(...changedPaths(before[index], after[index], `${path}[${index}]`));
    }
    return paths;
  }
  if (isObject(before) && isObject(after)) {
    const keys = [...new Set([...Object.keys(before), ...Object.keys(after)])];
    return keys.flatMap((key) => changedPaths(before[key], after[key], path ? `${path}.${key}` : key));
  }
  return [path || "(resource)"];
}
