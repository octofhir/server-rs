import { authInterceptor } from "./authInterceptor";
import { refreshAuthSession } from "./authSession";
import { assertFhirBundle, assertFhirResource } from "./guards";
import type {
  Bundle,
  CapabilityStatement,
  FhirBundle,
  FhirResource,
  HttpMethod,
  HttpRequestConfig,
  HttpResponse,
} from "./types";

export class HttpError extends Error {
  response: HttpResponse<unknown>;

  constructor(message: string, response: HttpResponse<unknown>) {
    super(message);
    this.name = "HttpError";
    this.response = response;
  }
}

type FhirSearchParams = Record<string, string | number | boolean | undefined>;
type FhirCreateResource<T extends FhirResource> = Partial<T> & Pick<T, "resourceType">;

export class FhirClient {
  private baseUrl: string;
  private defaultTimeout: number;
  private defaultHeaders: Record<string, string>;

  constructor(baseUrl = "", timeout = 30000) {
    this.baseUrl = baseUrl;
    this.defaultTimeout = timeout;
    this.defaultHeaders = {
      Accept: "application/fhir+json",
      "Content-Type": "application/fhir+json",
    };
  }

  private async request(config: HttpRequestConfig): Promise<HttpResponse<unknown>> {
    const {
      method,
      url,
      headers = {},
      data,
      timeout = this.defaultTimeout,
      credentials = "include",
    } = config;
    const fullUrl = url.startsWith("http") ? url : `${this.baseUrl}${url}`;

    // GET and HEAD requests cannot have a body
    const shouldIncludeBody = method !== "GET" && method !== "HEAD";

    const executeFetch = async (): Promise<Response> => {
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), timeout);

      try {
        return await fetch(fullUrl, {
          method,
          credentials,
          headers: {
            ...this.defaultHeaders,
            ...headers,
          },
          body: shouldIncludeBody && data ? JSON.stringify(data) : undefined,
          signal: controller.signal,
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
    const responseHeaders: Record<string, string> = {};
    response.headers.forEach((value, key) => {
      responseHeaders[key] = value;
    });

    // Parse response data. FHIR DELETE/empty responses are valid and should not
    // be forced through JSON.parse.
    let responseData: unknown;
    const contentType = response.headers.get("content-type");
    const rawBody = await response.text();

    if (!rawBody) {
      responseData = undefined;
    } else if (
      contentType?.includes("application/json") ||
      contentType?.includes("application/fhir+json")
    ) {
      responseData = JSON.parse(rawBody);
    } else {
      responseData = rawBody;
    }

    const result: HttpResponse<unknown> = {
      data: responseData,
      status: response.status,
      statusText: response.statusText,
      headers: responseHeaders,
      config,
    };

    if (!response.ok) {
      throw new HttpError(`HTTP ${response.status}: ${response.statusText}`, result);
    }

    return result;
  }

  // FHIR REST API methods
  async read<T extends FhirResource = FhirResource>(resourceType: string, id: string): Promise<T> {
    const response = await this.request({
      method: "GET",
      url: `/${resourceType}/${id}`,
    });
    return assertFhirResource(response.data, `read ${resourceType}/${id}`) as T;
  }

  async search<T extends FhirResource = FhirResource>(
    resourceType: string,
    params: FhirSearchParams = {}
  ): Promise<Bundle<T>> {
    const searchParams = new URLSearchParams();

    Object.entries(params).forEach(([key, value]) => {
      if (value === undefined) return;
      searchParams.set(key, String(value));
    });

    const queryString = searchParams.toString();
    const url = `/${resourceType}${queryString ? `?${queryString}` : ""}`;

    const response = await this.request({
      method: "GET",
      url,
    });

    return assertFhirBundle(response.data, `search ${resourceType}`) as Bundle<T>;
  }

  async create<T extends FhirResource = FhirResource>(resource: FhirCreateResource<T>): Promise<T> {
    const response = await this.request({
      method: "POST",
      url: `/${resource.resourceType}`,
      data: resource,
    });
    return assertFhirResource(response.data, `create ${resource.resourceType}`) as T;
  }

  async update<T extends FhirResource = FhirResource>(resource: T): Promise<T> {
    if (!resource.id) {
      throw new Error("Resource must have an ID for update");
    }

    const response = await this.request({
      method: "PUT",
      url: `/${resource.resourceType}/${resource.id}`,
      data: resource,
    });
    return assertFhirResource(response.data, `update ${resource.resourceType}/${resource.id}`) as T;
  }

  async delete(resourceType: string, id: string): Promise<void> {
    await this.request({
      method: "DELETE",
      url: `/${resourceType}/${id}`,
    });
  }

  async getCapabilities(): Promise<CapabilityStatement> {
    const response = await this.request({
      method: "GET",
      url: "/fhir/metadata",
    });
    const resource = assertFhirResource(response.data, "get capabilities") as CapabilityStatement;
    if (resource.resourceType !== "CapabilityStatement") {
      throw new Error("get capabilities: expected CapabilityStatement response");
    }
    return resource;
  }

  // Generic request method for custom operations
  async customRequest(
    config: Omit<HttpRequestConfig, "url"> & { url: string }
  ): Promise<HttpResponse<unknown>> {
    return this.request(config);
  }

  // Raw request method for REST console with timing
  async rawRequest(
    method: HttpMethod,
    path: string,
    body?: unknown,
    options?: { timeout?: number; includeCredentials?: boolean; headers?: Record<string, string> }
  ): Promise<HttpResponse<unknown> & { responseTime: number }> {
    const startTime = performance.now();
    const response = await this.request({
      method,
      url: path,
      data: body,
      timeout: options?.timeout,
      headers: options?.headers,
      credentials: options?.includeCredentials === false ? "omit" : "include",
    });
    const endTime = performance.now();
    return {
      ...response,
      responseTime: endTime - startTime,
    };
  }

  // Bundle navigation helpers
  async followLink(
    bundle: FhirBundle,
    relation: "first" | "prev" | "next" | "last"
  ): Promise<FhirBundle | null> {
    const link = bundle.link?.find((l) => l.relation === relation);
    if (!link?.url) {
      return null;
    }

    const response = await this.request({
      method: "GET",
      url: link.url,
    });

    return assertFhirBundle(response.data, `follow bundle link ${relation}`);
  }

  setBaseUrl(baseUrl: string): void {
    this.baseUrl = baseUrl;
  }

  setTimeout(timeout: number): void {
    this.defaultTimeout = timeout;
  }

  setDefaultHeaders(headers: Record<string, string>): void {
    this.defaultHeaders = { ...this.defaultHeaders, ...headers };
  }
}

// Internal resources (User, Role, Client, etc.) are accessed at root level without /fhir prefix
// Regular FHIR resources (Patient, Observation, etc.) are still under /fhir
const INTERNAL_RESOURCES = new Set([
  "User",
  "Role",
  "Client",
  "AccessPolicy",
  "IdentityProvider",
  "CustomOperation",
  "Session",
  "RefreshToken",
  "RevokedToken",
  "App",
  "AuthSession",
  // octofhir-ui IG
  "UserPreference",
  // octofhir-console IG
  "ConsoleCollection",
  "ConsoleSavedRequest",
  "ConsoleEnvironment",
  "ConsoleHistoryEntry",
]);

// Create a custom client that routes internal resources to root and others to /fhir
class OctoFhirClient extends FhirClient {
  private getBaseUrlForResource(resourceType: string): string {
    return INTERNAL_RESOURCES.has(resourceType) ? "" : "/fhir";
  }

  async read<T extends FhirResource = FhirResource>(resourceType: string, id: string): Promise<T> {
    const baseUrl = this.getBaseUrlForResource(resourceType);
    const response = await this.customRequest({
      method: "GET",
      url: `${baseUrl}/${resourceType}/${id}`,
    });
    return assertFhirResource(response.data, `read ${resourceType}/${id}`) as T;
  }

  async search<T extends FhirResource = FhirResource>(
    resourceType: string,
    params: Record<string, string | number> = {}
  ): Promise<Bundle<T>> {
    const baseUrl = this.getBaseUrlForResource(resourceType);
    const searchParams = new URLSearchParams();

    Object.entries(params).forEach(([key, value]) => {
      searchParams.set(key, String(value));
    });

    const queryString = searchParams.toString();
    const url = `${baseUrl}/${resourceType}${queryString ? `?${queryString}` : ""}`;

    const response = await this.customRequest({
      method: "GET",
      url,
    });

    return assertFhirBundle(response.data, `search ${resourceType}`) as Bundle<T>;
  }

  async create<T extends FhirResource = FhirResource>(resource: FhirCreateResource<T>): Promise<T> {
    const baseUrl = this.getBaseUrlForResource(resource.resourceType);
    const response = await this.customRequest({
      method: "POST",
      url: `${baseUrl}/${resource.resourceType}`,
      data: resource,
    });
    return assertFhirResource(response.data, `create ${resource.resourceType}`) as T;
  }

  async update<T extends FhirResource = FhirResource>(resource: T): Promise<T> {
    if (!resource.id) {
      throw new Error("Resource must have an ID for update");
    }

    const baseUrl = this.getBaseUrlForResource(resource.resourceType);
    const response = await this.customRequest({
      method: "PUT",
      url: `${baseUrl}/${resource.resourceType}/${resource.id}`,
      data: resource,
    });
    return assertFhirResource(response.data, `update ${resource.resourceType}/${resource.id}`) as T;
  }

  async delete(resourceType: string, id: string): Promise<void> {
    const baseUrl = this.getBaseUrlForResource(resourceType);
    await this.customRequest({
      method: "DELETE",
      url: `${baseUrl}/${resourceType}/${id}`,
    });
  }
}

// Default instance - uses custom routing logic
export const fhirClient = new OctoFhirClient("");
