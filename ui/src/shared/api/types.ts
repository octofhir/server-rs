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

// SQL execution types
export interface SqlRequest {
  query: string;
  /** Optional bind parameters for parameterized queries ($1, $2, etc.) */
  params?: SqlValue[];
}

export type SqlValue = string | number | boolean | null | Record<string, unknown>;

export interface SqlResponse {
  columns: string[];
  rows: SqlValue[][];
  rowCount: number;
  executionTimeMs: number;
}

// Auth types
export interface LoginRequest {
  grant_type: "password";
  client_id: string;
  username: string;
  password: string;
}

export interface TokenResponse {
  access_token: string;
  token_type: "Bearer";
  expires_in: number;
  scope?: string;
}

export interface UserInfo {
  sub: string;
  name?: string;
  preferred_username?: string;
  email?: string;
  fhirUser?: string;
  roles?: string[];
}

export interface AuthError {
  error: string;
  error_description?: string;
}

export interface LogoutResponse {
  success: boolean;
  message: string;
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
