import type {
  ActiveQueriesResponse,
  BuildInfo,
  CategorizedResourceTypesResponse,
  DropIndexResponse,
  GraphQLResponse,
  HealthResponse,
  HttpResponse,
  HttpMethod,
  InstallEvent,
  MaintenanceRequest,
  MaintenanceResponse,
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
import { isRecord } from "./guards";

interface RequestOptions {
  timeoutMs?: number;
}

/**
 * Custom error class that includes the parsed response body (e.g., OperationOutcome).
 */
export class ApiResponseError extends Error {
  status: number;
  statusText: string;
  responseData: unknown;

  constructor(message: string, status: number, statusText: string, responseData: unknown) {
    super(message);
    this.name = "ApiResponseError";
    this.status = status;
    this.statusText = statusText;
    this.responseData = responseData;
  }
}

function toHttpMethod(method: string | undefined): HttpMethod {
  switch (method) {
    case "POST":
    case "PUT":
    case "DELETE":
    case "PATCH":
    case "HEAD":
    case "OPTIONS":
      return method;
    case "GET":
    case undefined:
      return "GET";
    default:
      throw new Error(`Unsupported HTTP method: ${method}`);
  }
}

function isString(value: unknown): value is string {
  return typeof value === "string";
}

function isNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every(isString);
}

function isOptionalString(value: unknown): boolean {
  // `null` is how the server serializes an absent optional (serde None).
  return value == null || typeof value === "string";
}

function isOptionalNumber(value: unknown): boolean {
  return value == null || isNumber(value);
}

function assertResponse<T>(
  value: unknown,
  guard: (item: unknown) => item is T,
  context: string
): T {
  if (!guard(value)) {
    throw new Error(`${context}: invalid response`);
  }
  return value;
}

function isSuccessResponse(value: unknown): value is { success: boolean } {
  return isRecord(value) && typeof value.success === "boolean";
}

function isHealthResponse(value: unknown): value is HealthResponse {
  return (
    isRecord(value) &&
    (value.status === "ok" || value.status === "degraded" || value.status === "down") &&
    isOptionalString(value.details)
  );
}

function isBuildInfo(value: unknown): value is BuildInfo {
  return (
    isRecord(value) &&
    typeof value.serverVersion === "string" &&
    typeof value.commit === "string" &&
    typeof value.commitTimestamp === "string" &&
    isOptionalString(value.uiVersion)
  );
}

function isServerSettings(value: unknown): value is ServerSettings {
  return (
    isRecord(value) &&
    typeof value.fhirVersion === "string" &&
    isRecord(value.features) &&
    typeof value.features.sqlOnFhir === "boolean" &&
    typeof value.features.graphql === "boolean" &&
    typeof value.features.bulkExport === "boolean" &&
    typeof value.features.dbConsole === "boolean" &&
    typeof value.features.auth === "boolean" &&
    typeof value.features.cql === "boolean"
  );
}

function isCategorizedResourceTypesResponse(
  value: unknown
): value is CategorizedResourceTypesResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.types) &&
    value.types.every(
      (item) =>
        isRecord(item) &&
        typeof item.name === "string" &&
        (item.category === "fhir" || item.category === "system" || item.category === "custom") &&
        isOptionalString(item.url) &&
        typeof item.package === "string"
    ) &&
    isRecord(value.counts) &&
    isNumber(value.counts.all) &&
    isNumber(value.counts.fhir) &&
    isNumber(value.counts.system) &&
    isNumber(value.counts.custom)
  );
}

function isAutocompleteSuggestion(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    (value.kind === "resource" ||
      value.kind === "system-op" ||
      value.kind === "type-op" ||
      value.kind === "instance-op" ||
      value.kind === "api-endpoint") &&
    typeof value.label === "string" &&
    typeof value.path_template === "string" &&
    isStringArray(value.methods) &&
    isStringArray(value.placeholders) &&
    isOptionalString(value.description) &&
    isRecord(value.metadata) &&
    isOptionalString(value.metadata.resource_type) &&
    typeof value.metadata.affects_state === "boolean" &&
    typeof value.metadata.requires_body === "boolean" &&
    isOptionalString(value.metadata.category)
  );
}

function isModifierSuggestion(value: unknown): boolean {
  return isRecord(value) && typeof value.code === "string" && isOptionalString(value.description);
}

function isRestConsoleSearchParam(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.code === "string" &&
    typeof value.type === "string" &&
    isOptionalString(value.description) &&
    Array.isArray(value.modifiers) &&
    value.modifiers.every(isModifierSuggestion) &&
    isStringArray(value.comparators) &&
    isStringArray(value.targets) &&
    typeof value.is_common === "boolean"
  );
}

function isOperationCapabilityInfo(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.code === "string" &&
    typeof value.method === "string" &&
    isOptionalString(value.description) &&
    typeof value.affects_state === "boolean" &&
    isStringArray(value.resource_types)
  );
}

function isEnrichedSearchParam(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.code === "string" &&
    typeof value.param_type === "string" &&
    isOptionalString(value.description) &&
    Array.isArray(value.modifiers) &&
    value.modifiers.every(isModifierSuggestion) &&
    isStringArray(value.comparators) &&
    isStringArray(value.targets) &&
    Array.isArray(value.chains) &&
    value.chains.every(
      (chain) =>
        isRecord(chain) &&
        typeof chain.target_type === "string" &&
        isStringArray(chain.target_params)
    ) &&
    typeof value.is_common === "boolean"
  );
}

function isRestConsoleResponse(value: unknown): value is RestConsoleResponse {
  return (
    isRecord(value) &&
    typeof value.schema_version === "number" &&
    typeof value.fhir_version === "string" &&
    typeof value.base_path === "string" &&
    typeof value.generated_at === "string" &&
    isRecord(value.suggestions) &&
    Array.isArray(value.suggestions.resources) &&
    value.suggestions.resources.every(isAutocompleteSuggestion) &&
    Array.isArray(value.suggestions.system_operations) &&
    value.suggestions.system_operations.every(isAutocompleteSuggestion) &&
    Array.isArray(value.suggestions.type_operations) &&
    value.suggestions.type_operations.every(isAutocompleteSuggestion) &&
    Array.isArray(value.suggestions.instance_operations) &&
    value.suggestions.instance_operations.every(isAutocompleteSuggestion) &&
    Array.isArray(value.suggestions.api_endpoints) &&
    value.suggestions.api_endpoints.every(isAutocompleteSuggestion) &&
    isRecord(value.search_params) &&
    Object.values(value.search_params).every(
      (params) => Array.isArray(params) && params.every(isRestConsoleSearchParam)
    ) &&
    Array.isArray(value.resources) &&
    value.resources.every(
      (resource) =>
        isRecord(resource) &&
        typeof resource.resource_type === "string" &&
        Array.isArray(resource.search_params) &&
        resource.search_params.every(isEnrichedSearchParam) &&
        Array.isArray(resource.includes) &&
        resource.includes.every(
          (include) =>
            isRecord(include) &&
            typeof include.param_code === "string" &&
            isStringArray(include.target_types)
        ) &&
        Array.isArray(resource.rev_includes) &&
        resource.rev_includes.every(
          (include) =>
            isRecord(include) &&
            typeof include.param_code === "string" &&
            isStringArray(include.target_types)
        ) &&
        isStringArray(resource.sort_params) &&
        Array.isArray(resource.type_operations) &&
        resource.type_operations.every(isOperationCapabilityInfo) &&
        Array.isArray(resource.instance_operations) &&
        resource.instance_operations.every(isOperationCapabilityInfo)
    ) &&
    Array.isArray(value.system_operations) &&
    value.system_operations.every(isOperationCapabilityInfo) &&
    Array.isArray(value.special_params) &&
    value.special_params.every(
      (param) =>
        isRecord(param) &&
        typeof param.name === "string" &&
        isOptionalString(param.description) &&
        typeof param.supported === "boolean" &&
        isStringArray(param.examples)
    )
  );
}

function isSqlValue(value: unknown): value is SqlValue {
  return (
    value === null ||
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean" ||
    isRecord(value)
  );
}

function isSqlResponse(value: unknown): value is SqlResponse {
  return (
    isRecord(value) &&
    isStringArray(value.columns) &&
    Array.isArray(value.rows) &&
    value.rows.every((row) => Array.isArray(row) && row.every(isSqlValue)) &&
    isNumber(value.rowCount) &&
    isNumber(value.executionTimeMs)
  );
}

function isQueryHistoryResponse(value: unknown): value is QueryHistoryResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.entries) &&
    value.entries.every(
      (entry) =>
        isRecord(entry) &&
        typeof entry.id === "string" &&
        typeof entry.userId === "string" &&
        typeof entry.query === "string" &&
        isOptionalNumber(entry.executionTimeMs) &&
        isOptionalNumber(entry.rowCount) &&
        typeof entry.isError === "boolean" &&
        isOptionalString(entry.errorMessage) &&
        typeof entry.createdAt === "string"
    )
  );
}

function isTablesResponse(value: unknown): value is TablesResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.tables) &&
    value.tables.every(
      (table) =>
        isRecord(table) &&
        typeof table.schema === "string" &&
        typeof table.name === "string" &&
        typeof table.tableType === "string" &&
        isOptionalNumber(table.rowEstimate)
    )
  );
}

function isTableDetailResponse(value: unknown): value is TableDetailResponse {
  return (
    isRecord(value) &&
    typeof value.schema === "string" &&
    typeof value.name === "string" &&
    Array.isArray(value.columns) &&
    value.columns.every(
      (column) =>
        isRecord(column) &&
        typeof column.name === "string" &&
        typeof column.dataType === "string" &&
        typeof column.isNullable === "boolean" &&
        isOptionalString(column.defaultValue)
    ) &&
    Array.isArray(value.indexes) &&
    value.indexes.every(
      (index) =>
        isRecord(index) &&
        typeof index.name === "string" &&
        isStringArray(index.columns) &&
        typeof index.isUnique === "boolean" &&
        typeof index.isPrimary === "boolean" &&
        typeof index.indexType === "string" &&
        isOptionalNumber(index.sizeBytes)
    )
  );
}

function isActiveQueriesResponse(value: unknown): value is ActiveQueriesResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.queries) &&
    value.queries.every(
      (query) =>
        isRecord(query) &&
        isNumber(query.pid) &&
        isOptionalString(query.username) &&
        isOptionalString(query.database) &&
        isOptionalString(query.query) &&
        isOptionalString(query.state) &&
        isOptionalString(query.queryStart) &&
        isOptionalNumber(query.durationMs) &&
        isOptionalString(query.waitEventType) &&
        isOptionalString(query.waitEvent)
    )
  );
}

function isTerminateQueryResponse(value: unknown): value is TerminateQueryResponse {
  return (
    isRecord(value) && typeof value.success === "boolean" && typeof value.terminated === "boolean"
  );
}

function isDropIndexResponse(value: unknown): value is DropIndexResponse {
  return isRecord(value) && typeof value.success === "boolean" && typeof value.message === "string";
}

function isMaintenanceResponse(value: unknown): value is MaintenanceResponse {
  return (
    isRecord(value) &&
    typeof value.success === "boolean" &&
    typeof value.op === "string" &&
    typeof value.message === "string" &&
    isOptionalNumber(value.executionTimeMs)
  );
}

function isGraphQLResponse(value: unknown): value is GraphQLResponse {
  return (
    isRecord(value) &&
    (value.data === undefined || value.data === null || isRecord(value.data)) &&
    (value.errors === undefined ||
      (Array.isArray(value.errors) &&
        value.errors.every(
          (error) =>
            isRecord(error) &&
            typeof error.message === "string" &&
            (error.locations === undefined ||
              (Array.isArray(error.locations) &&
                error.locations.every(
                  (location) =>
                    isRecord(location) && isNumber(location.line) && isNumber(location.column)
                ))) &&
            (error.path === undefined ||
              (Array.isArray(error.path) &&
                error.path.every(
                  (path) => typeof path === "string" || typeof path === "number"
                ))) &&
            (error.extensions === undefined || isRecord(error.extensions))
        ))) &&
    (value.extensions === undefined || isRecord(value.extensions))
  );
}

function isOperationDefinition(value: unknown): value is OperationDefinition {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.name === "string" &&
    isOptionalString(value.description) &&
    typeof value.category === "string" &&
    isStringArray(value.methods) &&
    typeof value.path_pattern === "string" &&
    typeof value.public === "boolean" &&
    typeof value.module === "string" &&
    (value.app === undefined ||
      (isRecord(value.app) &&
        typeof value.app.id === "string" &&
        typeof value.app.name === "string"))
  );
}

function isOperationsResponse(value: unknown): value is OperationsResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.operations) &&
    value.operations.every(isOperationDefinition) &&
    isNumber(value.total)
  );
}

function isPackageListResponse(value: unknown): value is PackageListResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.packages) &&
    value.packages.every(
      (pkg) =>
        isRecord(pkg) &&
        typeof pkg.name === "string" &&
        typeof pkg.version === "string" &&
        isOptionalString(pkg.fhirVersion) &&
        isNumber(pkg.resourceCount) &&
        isOptionalString(pkg.installedAt)
    ) &&
    typeof value.serverFhirVersion === "string"
  );
}

function isPackageDetailResponse(value: unknown): value is PackageDetailResponse {
  return (
    isRecord(value) &&
    typeof value.name === "string" &&
    typeof value.version === "string" &&
    isOptionalString(value.fhirVersion) &&
    isOptionalString(value.description) &&
    isNumber(value.resourceCount) &&
    isOptionalString(value.installedAt) &&
    typeof value.isCompatible === "boolean" &&
    Array.isArray(value.resourceTypes) &&
    value.resourceTypes.every(
      (resource) =>
        isRecord(resource) && typeof resource.resourceType === "string" && isNumber(resource.count)
    )
  );
}

function isPackageResourcesResponse(value: unknown): value is PackageResourcesResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.resources) &&
    value.resources.every(
      (resource) =>
        isRecord(resource) &&
        isOptionalString(resource.id) &&
        isOptionalString(resource.url) &&
        isOptionalString(resource.name) &&
        isOptionalString(resource.version) &&
        typeof resource.resourceType === "string"
    ) &&
    isNumber(value.total)
  );
}

function isPackageLookupResponse(value: unknown): value is PackageLookupResponse {
  return (
    isRecord(value) &&
    typeof value.name === "string" &&
    isStringArray(value.versions) &&
    isStringArray(value.installedVersions)
  );
}

function isPackageSearchResponse(value: unknown): value is PackageSearchResponse {
  return (
    isRecord(value) &&
    typeof value.query === "string" &&
    Array.isArray(value.packages) &&
    value.packages.every(
      (pkg) =>
        isRecord(pkg) &&
        typeof pkg.name === "string" &&
        isStringArray(pkg.versions) &&
        isOptionalString(pkg.description) &&
        typeof pkg.latestVersion === "string"
    ) &&
    isNumber(value.total)
  );
}

function isPackageInstallResponse(value: unknown): value is PackageInstallResponse {
  return (
    isRecord(value) &&
    typeof value.success === "boolean" &&
    typeof value.name === "string" &&
    typeof value.version === "string" &&
    typeof value.fhirVersion === "string" &&
    isNumber(value.resourceCount) &&
    typeof value.message === "string"
  );
}

function isInstallEvent(value: unknown): value is InstallEvent {
  return isRecord(value) && typeof value.type === "string";
}

function readRequestHeaders(headers: HeadersInit | undefined): Record<string, string> | undefined {
  if (!headers) {
    return undefined;
  }

  if (headers instanceof Headers) {
    const result: Record<string, string> = {};
    headers.forEach((value, key) => {
      result[key] = value;
    });
    return result;
  }

  if (Array.isArray(headers)) {
    return Object.fromEntries(headers.map(([key, value]) => [key, value]));
  }

  return headers;
}

class ServerApiClient {
  private baseUrl: string;
  private defaultTimeout: number;

  constructor(baseUrl = "", timeout = 10000) {
    this.baseUrl = baseUrl;
    this.defaultTimeout = timeout;
  }

  private async request(
    endpoint: string,
    options: RequestInit = {},
    requestOptions: RequestOptions = {}
  ): Promise<HttpResponse<unknown>> {
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
    let data: unknown;
    const contentType = response.headers.get("content-type");
    const rawBody = await response.text();

    if (!rawBody) {
      data = undefined;
    } else if (
      contentType?.includes("application/json") ||
      contentType?.includes("application/fhir+json")
    ) {
      data = JSON.parse(rawBody);
    } else {
      data = rawBody;
    }

    const result: HttpResponse<unknown> = {
      data,
      status: response.status,
      statusText: response.statusText,
      headers,
      config: {
        method: toHttpMethod(options.method),
        url,
        headers: readRequestHeaders(options.headers),
        data: options.body,
      },
    };

    if (!response.ok) {
      throw new ApiResponseError(
        `HTTP ${response.status}: ${response.statusText}`,
        response.status,
        response.statusText,
        data
      );
    }

    return result;
  }

  async getHealth(): Promise<HealthResponse> {
    try {
      const response = await this.request("/api/health");
      return assertResponse(response.data, isHealthResponse, "getHealth");
    } catch (error) {
      return {
        status: "down",
        details: error instanceof Error ? error.message : "Unknown error",
      };
    }
  }

  async getBuildInfo(): Promise<BuildInfo> {
    const response = await this.request("/api/build-info");
    return assertResponse(response.data, isBuildInfo, "getBuildInfo");
  }

  async getSettings(): Promise<ServerSettings> {
    const response = await this.request("/api/settings");
    return assertResponse(response.data, isServerSettings, "getSettings");
  }

  async getResourceTypes(): Promise<string[]> {
    const response = await this.request("/api/resource-types");
    return assertResponse(response.data, isStringArray, "getResourceTypes");
  }

  /**
   * Get resource types with category information for UI grouping.
   * Categories: fhir, system, custom
   */
  async getResourceTypesCategorized(): Promise<CategorizedResourceTypesResponse> {
    const response = await this.request("/api/resource-types-categorized");
    return assertResponse(
      response.data,
      isCategorizedResourceTypesResponse,
      "getResourceTypesCategorized"
    );
  }

  /**
   * Get JSON Schema for a FHIR resource type.
   * Used for Monaco editor autocomplete and validation.
   */
  async getJsonSchema(resourceType: string): Promise<unknown> {
    const response = await this.request(`/api/json-schema/${encodeURIComponent(resourceType)}`);
    return response.data;
  }

  async getRestConsoleMetadata(): Promise<RestConsoleResponse> {
    const response = await this.request("/api/__introspect/rest-console");
    return assertResponse(response.data, isRestConsoleResponse, "getRestConsoleMetadata");
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
  async executeSql(query: string, params?: SqlValue[], timeoutMs?: number): Promise<SqlResponse> {
    const body: { query: string; params?: SqlValue[] } = { query };
    if (params && params.length > 0) {
      body.params = params;
    }
    const safeTimeoutMs =
      timeoutMs != null && Number.isFinite(timeoutMs) && timeoutMs > 0 ? timeoutMs : undefined;
    const response = await this.request(
      "/api/$sql",
      {
        method: "POST",
        body: JSON.stringify(body),
      },
      {
        timeoutMs: safeTimeoutMs,
      }
    );
    return assertResponse(response.data, isSqlResponse, "executeSql");
  }

  // =========================================================================
  // DB Console API
  // =========================================================================

  async getQueryHistory(): Promise<QueryHistoryResponse> {
    const response = await this.request("/api/db-console/history");
    return assertResponse(response.data, isQueryHistoryResponse, "getQueryHistory");
  }

  async saveQueryHistory(req: SaveHistoryRequest): Promise<{ success: boolean }> {
    const response = await this.request("/api/db-console/history", {
      method: "POST",
      body: JSON.stringify(req),
    });
    return assertResponse(response.data, isSuccessResponse, "saveQueryHistory");
  }

  async clearQueryHistory(): Promise<{ success: boolean }> {
    const response = await this.request("/api/db-console/history", {
      method: "DELETE",
    });
    return assertResponse(response.data, isSuccessResponse, "clearQueryHistory");
  }

  async getDbTables(): Promise<TablesResponse> {
    const response = await this.request("/api/db-console/tables");
    return assertResponse(response.data, isTablesResponse, "getDbTables");
  }

  async getTableDetail(schema: string, table: string): Promise<TableDetailResponse> {
    const response = await this.request(
      `/api/db-console/tables/${encodeURIComponent(schema)}/${encodeURIComponent(table)}`
    );
    return assertResponse(response.data, isTableDetailResponse, "getTableDetail");
  }

  async getActiveQueries(): Promise<ActiveQueriesResponse> {
    const response = await this.request("/api/db-console/active-queries");
    return assertResponse(response.data, isActiveQueriesResponse, "getActiveQueries");
  }

  async terminateQuery(req: TerminateQueryRequest): Promise<TerminateQueryResponse> {
    const response = await this.request("/api/db-console/terminate-query", {
      method: "POST",
      body: JSON.stringify(req),
    });
    return assertResponse(response.data, isTerminateQueryResponse, "terminateQuery");
  }

  async dropIndex(schema: string, indexName: string): Promise<DropIndexResponse> {
    const response = await this.request(
      `/api/db-console/indexes/${encodeURIComponent(schema)}/${encodeURIComponent(indexName)}`,
      { method: "DELETE" }
    );
    return assertResponse(response.data, isDropIndexResponse, "dropIndex");
  }

  async runMaintenance(
    schema: string,
    table: string,
    req: MaintenanceRequest
  ): Promise<MaintenanceResponse> {
    const response = await this.request(
      `/api/db-console/maintenance/${encodeURIComponent(schema)}/${encodeURIComponent(table)}`,
      { method: "POST", body: JSON.stringify(req) }
    );
    return assertResponse(response.data, isMaintenanceResponse, "runMaintenance");
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
    operationName?: string
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
    const response = await this.request("/$graphql", {
      method: "POST",
      body: JSON.stringify(body),
    });
    return assertResponse(response.data, isGraphQLResponse, "executeGraphQL");
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
    const response = await this.request(url);
    return assertResponse(response.data, isOperationsResponse, "getOperations");
  }

  /**
   * Get a single operation by ID.
   */
  async getOperation(id: string): Promise<OperationDefinition> {
    const response = await this.request(`/api/operations/${encodeURIComponent(id)}`);
    return assertResponse(response.data, isOperationDefinition, "getOperation");
  }

  /**
   * Update an operation (public flag, description).
   * Requires admin permissions.
   */
  async updateOperation(id: string, update: OperationUpdateRequest): Promise<OperationDefinition> {
    const response = await this.request(`/api/operations/${encodeURIComponent(id)}`, {
      method: "PATCH",
      body: JSON.stringify(update),
    });
    return assertResponse(response.data, isOperationDefinition, "updateOperation");
  }

  // ============ Package Management API ============

  /**
   * List all installed FHIR packages.
   */
  async getPackages(): Promise<PackageListResponse> {
    const response = await this.request("/api/packages");
    return assertResponse(response.data, isPackageListResponse, "getPackages");
  }

  /**
   * Get details for a specific package.
   */
  async getPackageDetails(name: string, version: string): Promise<PackageDetailResponse> {
    const response = await this.request(
      `/api/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}`
    );
    return assertResponse(response.data, isPackageDetailResponse, "getPackageDetails");
  }

  /**
   * List resources in a package with optional filtering.
   */
  async getPackageResources(
    name: string,
    version: string,
    params?: { resourceType?: string; limit?: number; offset?: number }
  ): Promise<PackageResourcesResponse> {
    const queryParams = new URLSearchParams();
    if (params?.resourceType) queryParams.set("resource_type", params.resourceType);
    if (params?.limit) queryParams.set("limit", String(params.limit));
    if (params?.offset) queryParams.set("offset", String(params.offset));
    const queryString = queryParams.toString();
    const url = `/api/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}/resources${queryString ? `?${queryString}` : ""}`;
    const response = await this.request(url);
    return assertResponse(response.data, isPackageResourcesResponse, "getPackageResources");
  }

  /**
   * Get full content of a specific resource from a package.
   */
  async getPackageResourceContent(
    name: string,
    version: string,
    resourceUrl: string
  ): Promise<unknown> {
    const response = await this.request(
      `/api/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}/resources/${encodeURIComponent(resourceUrl)}`
    );
    return response.data;
  }

  /**
   * Get FHIRSchema for a resource from a package.
   */
  async getPackageFhirSchema(name: string, version: string, resourceUrl: string): Promise<unknown> {
    const response = await this.request(
      `/api/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}/fhirschema/${encodeURIComponent(resourceUrl)}`
    );
    return response.data;
  }

  /**
   * Lookup available versions for a package from the FHIR registry.
   */
  async lookupPackage(name: string): Promise<PackageLookupResponse> {
    const response = await this.request(`/api/packages/lookup/${encodeURIComponent(name)}`);
    return assertResponse(response.data, isPackageLookupResponse, "lookupPackage");
  }

  /**
   * Search for packages in the FHIR registry.
   * Supports partial matching (ILIKE) - spaces in the query are treated as wildcards.
   */
  async searchPackages(query: string): Promise<PackageSearchResponse> {
    const response = await this.request(`/api/packages/search?q=${encodeURIComponent(query)}`);
    return assertResponse(response.data, isPackageSearchResponse, "searchPackages");
  }

  /**
   * Install a package from the FHIR registry.
   */
  async installPackage(request: PackageInstallRequest): Promise<PackageInstallResponse> {
    const response = await this.request("/api/packages/install", {
      method: "POST",
      body: JSON.stringify(request),
    });
    return assertResponse(response.data, isPackageInstallResponse, "installPackage");
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
    onComplete?: () => void
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
                if (!isInstallEvent(data)) {
                  continue;
                }

                onEvent(data);

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
