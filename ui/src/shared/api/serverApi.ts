import type {
  ActiveQueriesResponse,
  BuildInfo,
  CategorizedResourceTypesResponse,
  DropIndexResponse,
  GraphQLResponse,
  HealthResponse,
  HttpResponse,
  OperationDefinition,
  OperationsResponse,
  OperationUpdateRequest,
  PackageDetailResponse,
  PackageInstallRequest,
  PackageInstallResponse,
  PackageListResponse,
  PackageLookupResponse,
  PackageResourcesResponse,
  PackageSearchResponse,
  QueryHistoryResponse,
  RestConsoleResponse,
  SaveHistoryRequest,
  ServerSettings,
  SqlResponse,
  SqlValue,
  TableDetailResponse,
  TablesResponse,
  TerminateQueryRequest,
  TerminateQueryResponse,
} from "./types";
import { authInterceptor } from "./authInterceptor";
import { refreshAuthSession } from "./authSession";

interface RequestOptions {
  timeoutMs?: number;
}

/**
 * Custom error class that includes the parsed response body (e.g., OperationOutcome).
 */
export class ApiResponseError extends Error {
  constructor(
    message: string,
    public status: number,
    public statusText: string,
    public responseData: any,
  ) {
    super(message);
    this.name = "ApiResponseError";
  }
}

class ServerApiClient {
  private baseUrl: string;
  private defaultTimeout: number;

  constructor(baseUrl = "", timeout = 10000) {
    this.baseUrl = baseUrl;
    this.defaultTimeout = timeout;
  }

  private async request<T>(
    endpoint: string,
    options: RequestInit = {},
    requestOptions: RequestOptions = {},
  ): Promise<HttpResponse<T>> {
    const url = `${this.baseUrl}${endpoint}`;
    const timeoutMs = requestOptions.timeoutMs ?? this.defaultTimeout;

    const executeFetch = async (): Promise<Response> => {
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), timeoutMs);

      try {
        return await fetch(url, {
          ...options,
          signal: controller.signal,
          credentials: "include", // Include cookies for auth
          headers: {
            "Content-Type": "application/json",
            ...options.headers,
          },
        });
      } catch (error) {
        if (error instanceof Error && error.name === "AbortError") {
          throw new Error("Request timeout");
        }
        throw error;
      } finally {
        clearTimeout(timeoutId);
      }
    };

    let response = await executeFetch();

    if (response.status === 401 || response.status === 403) {
      const refreshed = await refreshAuthSession(true);
      if (refreshed) {
        response = await executeFetch();
      }
    }

    if (response.status === 401 || response.status === 403) {
      authInterceptor.handleResponse(response);
    }

    // Parse response headers
    const headers: Record<string, string> = {};
    response.headers.forEach((value, key) => {
      headers[key] = value;
    });

    // Parse response data
    let data: T;
    const contentType = response.headers.get("content-type");

    if (contentType?.includes("application/json") || contentType?.includes("application/fhir+json")) {
      data = await response.json();
    } else {
      data = (await response.text()) as unknown as T;
    }

    const result: HttpResponse<T> = {
      data,
      status: response.status,
      statusText: response.statusText,
      headers,
      config: {
        method: (options.method || "GET") as any,
        url,
        headers: options.headers as Record<string, string>,
        data: options.body,
      },
    };

    if (!response.ok) {
      throw new ApiResponseError(
        `HTTP ${response.status}: ${response.statusText}`,
        response.status,
        response.statusText,
        data,
      );
    }

    return result;
  }

  async getHealth(): Promise<HealthResponse> {
    try {
      const response = await this.request<HealthResponse>("/api/health");
      return response.data;
    } catch (error) {
      return {
        status: "down",
        details: error instanceof Error ? error.message : "Unknown error",
      };
    }
  }

  async getBuildInfo(): Promise<BuildInfo> {
    const response = await this.request<BuildInfo>("/api/build-info");
    return response.data;
  }

  async getSettings(): Promise<ServerSettings> {
    const response = await this.request<ServerSettings>("/api/settings");
    return response.data;
  }

  async getResourceTypes(): Promise<string[]> {
    const response = await this.request<string[]>("/api/resource-types");
    return response.data;
  }

  /**
   * Get resource types with category information for UI grouping.
   * Categories: fhir, system, custom
   */
  async getResourceTypesCategorized(): Promise<CategorizedResourceTypesResponse> {
    const response = await this.request<CategorizedResourceTypesResponse>(
      "/api/resource-types-categorized"
    );
    return response.data;
  }

  /**
   * Get JSON Schema for a FHIR resource type.
   * Used for Monaco editor autocomplete and validation.
   */
  async getJsonSchema(resourceType: string): Promise<unknown> {
    const response = await this.request<unknown>(
      `/api/json-schema/${encodeURIComponent(resourceType)}`
    );
    return response.data;
  }

  async getRestConsoleMetadata(): Promise<RestConsoleResponse> {
    const response = await this.request<RestConsoleResponse>("/api/__introspect/rest-console");
    return response.data;
  }

  /**
   * Execute a SQL query against the database.
   * Requires DB console to be enabled in server configuration.
   *
   * @param query - The SQL query to execute (supports $1, $2, etc. placeholders)
   * @param params - Optional bind parameters for parameterized queries
   * @returns SQL execution result with columns, rows, and timing info
   * @throws Error if query fails or is not allowed by sql_mode policy
   *
   * @example
   * // Simple query
   * await executeSql("SELECT * FROM patient LIMIT 10");
   *
   * @example
   * // Parameterized query (safe from SQL injection)
   * await executeSql(
   *   "SELECT * FROM patient WHERE id = $1 AND status = $2",
   *   ["123", "active"]
   * );
   */
  async executeSql(
    query: string,
    params?: SqlValue[],
    timeoutMs?: number,
  ): Promise<SqlResponse> {
    const body: { query: string; params?: SqlValue[] } = { query };
    if (params && params.length > 0) {
      body.params = params;
    }
    const safeTimeoutMs =
      timeoutMs != null && Number.isFinite(timeoutMs) && timeoutMs > 0
        ? timeoutMs
        : undefined;
    const response = await this.request<SqlResponse>("/api/$sql", {
      method: "POST",
      body: JSON.stringify(body),
    }, {
      timeoutMs: safeTimeoutMs,
    });
    return response.data;
  }

  // =========================================================================
  // DB Console API
  // =========================================================================

  async getQueryHistory(): Promise<QueryHistoryResponse> {
    const response = await this.request<QueryHistoryResponse>("/api/db-console/history");
    return response.data;
  }

  async saveQueryHistory(req: SaveHistoryRequest): Promise<{ success: boolean }> {
    const response = await this.request<{ success: boolean }>("/api/db-console/history", {
      method: "POST",
      body: JSON.stringify(req),
    });
    return response.data;
  }

  async clearQueryHistory(): Promise<{ success: boolean }> {
    const response = await this.request<{ success: boolean }>("/api/db-console/history", {
      method: "DELETE",
    });
    return response.data;
  }

  async getDbTables(): Promise<TablesResponse> {
    const response = await this.request<TablesResponse>("/api/db-console/tables");
    return response.data;
  }

  async getTableDetail(schema: string, table: string): Promise<TableDetailResponse> {
    const response = await this.request<TableDetailResponse>(
      `/api/db-console/tables/${encodeURIComponent(schema)}/${encodeURIComponent(table)}`,
    );
    return response.data;
  }

  async getActiveQueries(): Promise<ActiveQueriesResponse> {
    const response = await this.request<ActiveQueriesResponse>("/api/db-console/active-queries");
    return response.data;
  }

  async terminateQuery(req: TerminateQueryRequest): Promise<TerminateQueryResponse> {
    const response = await this.request<TerminateQueryResponse>("/api/db-console/terminate-query", {
      method: "POST",
      body: JSON.stringify(req),
    });
    return response.data;
  }

  async dropIndex(schema: string, indexName: string): Promise<DropIndexResponse> {
    const response = await this.request<DropIndexResponse>(
      `/api/db-console/indexes/${encodeURIComponent(schema)}/${encodeURIComponent(indexName)}`,
      { method: "DELETE" },
    );
    return response.data;
  }

  /**
   * Execute a GraphQL query against the FHIR GraphQL endpoint.
   *
   * @param query - The GraphQL query to execute
   * @param variables - Optional variables for the query
   * @param operationName - Optional operation name when query contains multiple operations
   * @returns GraphQL response with data and/or errors
   *
   * @example
   * // Simple query
   * await executeGraphQL("{ Patient(_id: \"123\") { id name { family } } }");
   *
   * @example
   * // Query with variables
   * await executeGraphQL(
   *   "query GetPatient($id: String!) { Patient(_id: $id) { id } }",
   *   { id: "123" },
   *   "GetPatient"
   * );
   */
  async executeGraphQL(
    query: string,
    variables?: Record<string, unknown>,
    operationName?: string,
  ): Promise<GraphQLResponse> {
    const body: { query: string; variables?: Record<string, unknown>; operationName?: string } = {
      query,
    };
    if (variables) {
      body.variables = variables;
    }
    if (operationName) {
      body.operationName = operationName;
    }
    const response = await this.request<GraphQLResponse>("/$graphql", {
      method: "POST",
      body: JSON.stringify(body),
    });
    return response.data;
  }

  /**
   * Fetch the GraphQL schema using introspection.
   * Useful for providing autocomplete and documentation.
   */
  async getGraphQLSchema(): Promise<GraphQLResponse> {
    const introspectionQuery = `
      query IntrospectionQuery {
        __schema {
          queryType { name }
          mutationType { name }
          types {
            ...FullType
          }
        }
      }
      fragment FullType on __Type {
        kind
        name
        description
        fields(includeDeprecated: true) {
          name
          description
          args {
            ...InputValue
          }
          type {
            ...TypeRef
          }
          isDeprecated
          deprecationReason
        }
        inputFields {
          ...InputValue
        }
        enumValues(includeDeprecated: true) {
          name
          description
          isDeprecated
          deprecationReason
        }
      }
      fragment InputValue on __InputValue {
        name
        description
        type {
          ...TypeRef
        }
        defaultValue
      }
      fragment TypeRef on __Type {
        kind
        name
        ofType {
          kind
          name
          ofType {
            kind
            name
            ofType {
              kind
              name
              ofType {
                kind
                name
              }
            }
          }
        }
      }
    `;
    return this.executeGraphQL(introspectionQuery);
  }

  /**
   * Get all server operations.
   * Operations represent discrete API endpoints that can be targeted by access policies.
   *
   * @param category - Optional filter by category (fhir, graphql, system, auth, ui, api)
   * @param module - Optional filter by module
   * @param publicOnly - Optional filter to only show public operations
   */
  async getOperations(filters?: {
    category?: string;
    module?: string;
    public?: boolean;
  }): Promise<OperationsResponse> {
    const params = new URLSearchParams();
    if (filters?.category) params.set("category", filters.category);
    if (filters?.module) params.set("module", filters.module);
    if (filters?.public !== undefined) params.set("public", String(filters.public));
    const queryString = params.toString();
    const url = `/api/operations${queryString ? `?${queryString}` : ""}`;
    const response = await this.request<OperationsResponse>(url);
    return response.data;
  }

  /**
   * Get a single operation by ID.
   */
  async getOperation(id: string): Promise<OperationDefinition> {
    const response = await this.request<OperationDefinition>(`/api/operations/${encodeURIComponent(id)}`);
    return response.data;
  }

  /**
   * Update an operation (public flag, description).
   * Requires admin permissions.
   */
  async updateOperation(id: string, update: OperationUpdateRequest): Promise<OperationDefinition> {
    const response = await this.request<OperationDefinition>(`/api/operations/${encodeURIComponent(id)}`, {
      method: "PATCH",
      body: JSON.stringify(update),
    });
    return response.data;
  }

  // ============ Package Management API ============

  /**
   * List all installed FHIR packages.
   */
  async getPackages(): Promise<PackageListResponse> {
    const response = await this.request<PackageListResponse>("/api/packages");
    return response.data;
  }

  /**
   * Get details for a specific package.
   */
  async getPackageDetails(name: string, version: string): Promise<PackageDetailResponse> {
    const response = await this.request<PackageDetailResponse>(
      `/api/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}`,
    );
    return response.data;
  }

  /**
   * List resources in a package with optional filtering.
   */
  async getPackageResources(
    name: string,
    version: string,
    params?: { resourceType?: string; limit?: number; offset?: number },
  ): Promise<PackageResourcesResponse> {
    const queryParams = new URLSearchParams();
    if (params?.resourceType) queryParams.set("resource_type", params.resourceType);
    if (params?.limit) queryParams.set("limit", String(params.limit));
    if (params?.offset) queryParams.set("offset", String(params.offset));
    const queryString = queryParams.toString();
    const url = `/api/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}/resources${queryString ? `?${queryString}` : ""}`;
    const response = await this.request<PackageResourcesResponse>(url);
    return response.data;
  }

  /**
   * Get full content of a specific resource from a package.
   */
  async getPackageResourceContent(name: string, version: string, resourceUrl: string): Promise<unknown> {
    const response = await this.request<unknown>(
      `/api/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}/resources/${encodeURIComponent(resourceUrl)}`,
    );
    return response.data;
  }

  /**
   * Get FHIRSchema for a resource from a package.
   */
  async getPackageFhirSchema(name: string, version: string, resourceUrl: string): Promise<unknown> {
    const response = await this.request<unknown>(
      `/api/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}/fhirschema/${encodeURIComponent(resourceUrl)}`,
    );
    return response.data;
  }

  /**
   * Lookup available versions for a package from the FHIR registry.
   */
  async lookupPackage(name: string): Promise<PackageLookupResponse> {
    const response = await this.request<PackageLookupResponse>(
      `/api/packages/lookup/${encodeURIComponent(name)}`,
    );
    return response.data;
  }

  /**
   * Search for packages in the FHIR registry.
   * Supports partial matching (ILIKE) - spaces in the query are treated as wildcards.
   */
  async searchPackages(query: string): Promise<PackageSearchResponse> {
    const response = await this.request<PackageSearchResponse>(
      `/api/packages/search?q=${encodeURIComponent(query)}`,
    );
    return response.data;
  }

  /**
   * Install a package from the FHIR registry.
   */
  async installPackage(request: PackageInstallRequest): Promise<PackageInstallResponse> {
    const response = await this.request<PackageInstallResponse>("/api/packages/install", {
      method: "POST",
      body: JSON.stringify(request),
    });
    return response.data;
  }

  /**
   * Install a package from the FHIR registry with SSE progress streaming.
   * Returns an EventSource that emits InstallEvent objects.
   *
   * @param request - Package name and version to install
   * @param onEvent - Callback for each progress event
   * @param onError - Callback for errors
   * @param onComplete - Callback when installation completes
   * @returns A function to abort the installation
   *
   * @example
   * const abort = serverApi.installPackageWithProgress(
   *   { name: "hl7.fhir.us.core", version: "6.1.0" },
   *   (event) => console.log("Progress:", event),
   *   (error) => console.error("Error:", error),
   *   () => console.log("Complete!")
   * );
   * // To abort: abort();
   */
  installPackageWithProgress(
    request: PackageInstallRequest,
    onEvent: (event: import("./types").InstallEvent) => void,
    onError?: (error: Error) => void,
    onComplete?: () => void,
  ): () => void {
    const controller = new AbortController();
    const url = `${this.baseUrl}/api/packages/install/stream`;

    // Use fetch with streaming response
    fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Accept: "text/event-stream",
      },
      body: JSON.stringify(request),
      signal: controller.signal,
      credentials: "include",
    })
      .then(async (response) => {
        if (!response.ok) {
          const errorText = await response.text();
          throw new Error(`HTTP ${response.status}: ${errorText}`);
        }

        const reader = response.body?.getReader();
        if (!reader) {
          throw new Error("No response body");
        }

        const decoder = new TextDecoder();
        let buffer = "";

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          buffer += decoder.decode(value, { stream: true });

          // Parse SSE events from buffer
          const lines = buffer.split("\n");
          buffer = lines.pop() || ""; // Keep incomplete line in buffer

          for (const line of lines) {
            if (line.startsWith("data: ")) {
              try {
                const data = JSON.parse(line.slice(6));
                onEvent(data as import("./types").InstallEvent);

                // Check if this is the final event
                if (data.type === "completed" || data.type === "error") {
                  onComplete?.();
                  return;
                }
              } catch {
                // Ignore parse errors for non-JSON lines
              }
            }
          }
        }

        onComplete?.();
      })
      .catch((error) => {
        if (error.name !== "AbortError") {
          onError?.(error);
        }
      });

    // Return abort function
    return () => controller.abort();
  }

  setBaseUrl(baseUrl: string): void {
    this.baseUrl = baseUrl;
  }

  setTimeout(timeout: number): void {
    this.defaultTimeout = timeout;
  }
}

// Default instance
export const serverApi = new ServerApiClient();
export { ServerApiClient };
