// Server API types
export interface HealthResponse {
  status: "ok" | "degraded" | "down";
  details?: string;
}

export interface BuildInfo {
  serverVersion: string;
  commit: string;
  commitTimestamp: string;
  uiVersion?: string;
}

// FHIR types (minimal)
export interface FhirResource {
  resourceType: string;
  id?: string;
  meta?: {
    versionId?: string;
    lastUpdated?: string;
  };
  [key: string]: any;
}

export interface FhirBundle {
  resourceType: "Bundle";
  id?: string;
  type: string;
  total?: number;
  link?: Array<{
    relation: string;
    url: string;
  }>;
  entry?: Array<{
    resource: FhirResource;
    fullUrl?: string;
  }>;
}

export interface FhirOperationOutcome {
  resourceType: "OperationOutcome";
  issue: Array<{
    severity: "fatal" | "error" | "warning" | "information";
    code: string;
    diagnostics?: string;
    location?: string[];
  }>;
}

// HTTP types
export type HttpMethod = "GET" | "POST" | "PUT" | "DELETE" | "PATCH";

export interface HttpRequestConfig {
  method: HttpMethod;
  url: string;
  headers?: Record<string, string>;
  data?: any;
  timeout?: number;
}

export interface HttpResponse<T = any> {
  data: T;
  status: number;
  statusText: string;
  headers: Record<string, string>;
  config: HttpRequestConfig;
}
