import type { FhirBundle, FhirResource, CapabilityStatement } from "@/shared/api";

export interface ResourceBrowserState {
  selectedResourceType: string | null;
  selectedResourceId: string | null;
  resources: FhirResource[];
  bundle: FhirBundle | null;
  loading: boolean;
  error: string | null;
  searchParams: Record<string, string>;
  page: number;
  pageSize: number;
}

export interface FhirState {
  capabilities: CapabilityStatement | null;
  resourceTypes: string[];
  loading: boolean;
  error: string | null;
}
