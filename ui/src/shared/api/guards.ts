import type { FhirBundle, FhirResource } from "./types";

export type FhirResourceGuard<T extends FhirResource> = (value: unknown) => value is T;

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function isFhirResource(value: unknown): value is FhirResource {
  return isRecord(value) && typeof value.resourceType === "string" && value.resourceType.length > 0;
}

export function isFhirBundle(value: unknown): value is FhirBundle {
  return isFhirResource(value) && value.resourceType === "Bundle";
}

export function assertFhirResource(
  value: unknown,
  context: string,
): FhirResource;
export function assertFhirResource<T extends FhirResource>(
  value: unknown,
  context: string,
  guard: FhirResourceGuard<T>,
): T;
export function assertFhirResource<T extends FhirResource>(
  value: unknown,
  context: string,
  guard?: FhirResourceGuard<T>,
): FhirResource | T {
  if (!isFhirResource(value)) {
    throw new Error(`${context}: expected FHIR resource response`);
  }

  if (guard && !guard(value)) {
    throw new Error(`${context}: unexpected FHIR resource shape`);
  }

  return value;
}

export function assertFhirBundle(
  value: unknown,
  context: string,
): FhirBundle;
export function assertFhirBundle<T extends FhirResource>(
  value: unknown,
  context: string,
  guard: FhirResourceGuard<T>,
): FhirBundle<T>;
export function assertFhirBundle<T extends FhirResource>(
  value: unknown,
  context: string,
  guard?: FhirResourceGuard<T>,
): FhirBundle | FhirBundle<T> {
  if (!isFhirBundle(value)) {
    throw new Error(`${context}: expected FHIR Bundle response`);
  }

  if (!guard) {
    return value;
  }

  const entry = value.entry?.map((bundleEntry, index) => {
    if (!bundleEntry.resource) {
      return bundleEntry;
    }

    if (!guard(bundleEntry.resource)) {
      throw new Error(`${context}: unexpected resource shape at entry ${index}`);
    }

    return {
      ...bundleEntry,
      resource: bundleEntry.resource,
    };
  });

  return {
    ...value,
    entry,
  };
}

export function getBundleResources<T extends FhirResource = FhirResource>(
  bundle: Pick<FhirBundle, "entry"> | null | undefined,
): T[];
export function getBundleResources<T extends FhirResource>(
  bundle: Pick<FhirBundle, "entry"> | null | undefined,
  guard: FhirResourceGuard<T>,
): T[];
export function getBundleResources<T extends FhirResource>(
  bundle: Pick<FhirBundle, "entry"> | null | undefined,
  guard?: FhirResourceGuard<T>,
): T[] {
  if (!bundle?.entry) {
    return [];
  }

  const resources = bundle.entry.map((entry) => entry.resource).filter(isFhirResource);
  return (guard ? resources.filter(guard) : resources) as T[];
}
