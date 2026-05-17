import type { FhirBundle, FhirResource } from "./types";

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function isFhirResource(value: unknown): value is FhirResource {
  return isRecord(value) && typeof value.resourceType === "string" && value.resourceType.length > 0;
}

export function isFhirBundle(value: unknown): value is FhirBundle {
  return isFhirResource(value) && value.resourceType === "Bundle";
}

export function assertFhirResource<T extends FhirResource = FhirResource>(
  value: unknown,
  context: string,
): T {
  if (!isFhirResource(value)) {
    throw new Error(`${context}: expected FHIR resource response`);
  }
  return value as T;
}

export function assertFhirBundle<T extends FhirResource = FhirResource>(
  value: unknown,
  context: string,
): FhirBundle<T> {
  if (!isFhirBundle(value)) {
    throw new Error(`${context}: expected FHIR Bundle response`);
  }
  return value as FhirBundle<T>;
}

export function getBundleResources<T extends FhirResource = FhirResource>(
  bundle: Pick<FhirBundle, "entry"> | null | undefined,
): T[] {
  return (bundle?.entry?.map((entry) => entry.resource).filter(isFhirResource) as T[] | undefined) ?? [];
}
