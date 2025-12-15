import type {
  BuildInfo,
  GraphQLResponse,
  HealthResponse,
  HttpResponse,
  OperationDefinition,
  OperationsResponse,
  OperationUpdateRequest,
  RestConsoleResponse,
  SqlResponse,
  SqlValue,
} from "./types";
import { authInterceptor } from "./authInterceptor";

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

  private async request<T>(endpoint: string, options: RequestInit = {}): Promise<HttpResponse<T>> {
    const url = `${this.baseUrl}${endpoint}`;
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.defaultTimeout);

    try {
      const response = await fetch(url, {
        ...options,
        signal: controller.signal,
        credentials: "include", // Include cookies for auth
        headers: {
          "Content-Type": "application/json",
          ...options.headers,
        },
      });

      clearTimeout(timeoutId);

      // Check for auth errors (401/403) and notify interceptor
      authInterceptor.handleResponse(response);

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
    } catch (error) {
      clearTimeout(timeoutId);

      if (error instanceof Error && error.name === "AbortError") {
        throw new Error("Request timeout");
      }

      throw error;
    }
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

  async getResourceTypes(): Promise<string[]> {
    const response = await this.request<string[]>("/api/resource-types");
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
  async executeSql(query: string, params?: SqlValue[]): Promise<SqlResponse> {
    const body: { query: string; params?: SqlValue[] } = { query };
    if (params && params.length > 0) {
      body.params = params;
    }
    const response = await this.request<SqlResponse>("/api/$sql", {
      method: "POST",
      body: JSON.stringify(body),
    });
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
