// FHIR Gateway Resource Types

export type HttpMethod = "GET" | "POST" | "PUT" | "DELETE" | "PATCH";

export type OperationType = "proxy" | "sql" | "fhirpath" | "handler";

export type AuthenticationType = "none" | "bearer" | "basic" | "api-key";

// App Resource
export interface App {
  resourceType: "App";
  id?: string;
  meta?: {
    versionId?: string;
    lastUpdated?: string;
  };
  name: string;
  description?: string;
  basePath: string;
  active: boolean;
  authentication?: {
    type: AuthenticationType;
    required: boolean;
  };
}

// CustomOperation Resource
export interface CustomOperation {
  resourceType: "CustomOperation";
  id?: string;
  meta?: {
    versionId?: string;
    lastUpdated?: string;
  };
  app: {
    reference: string; // Reference to App resource (e.g., "App/123")
    display?: string;
  };
  path: string;
  method: HttpMethod;
  type: OperationType;
  active: boolean;
  description?: string;
  config?: ProxyConfig | SqlConfig | FhirPathConfig | HandlerConfig;
}

// Type-specific configurations
export interface ProxyConfig {
  url: string;
  timeout?: number;
  headers?: Record<string, string>;
  forwardAuth?: boolean;
}

export interface SqlConfig {
  query: string;
}

export interface FhirPathConfig {
  expression: string;
}

export interface HandlerConfig {
  name: string;
}

// Helper type for creating new resources
export type NewApp = Omit<App, "id" | "meta">;
export type NewCustomOperation = Omit<CustomOperation, "id" | "meta">;

// FHIR Bundle type for search results
export interface Bundle<T = any> {
  resourceType: "Bundle";
  type: string;
  total?: number;
  entry?: Array<{
    resource: T;
    fullUrl?: string;
  }>;
  link?: Array<{
    relation: string;
    url: string;
  }>;
}
