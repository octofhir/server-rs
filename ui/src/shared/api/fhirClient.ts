import type { FhirBundle, FhirResource, HttpRequestConfig, HttpResponse } from "./types";

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

  private async request<T = any>(config: HttpRequestConfig): Promise<HttpResponse<T>> {
    const { method, url, headers = {}, data, timeout = this.defaultTimeout } = config;
    const fullUrl = url.startsWith("http") ? url : `${this.baseUrl}${url}`;

    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), timeout);

    try {
      const response = await fetch(fullUrl, {
        method,
        credentials: "include", // Include cookies for auth
        headers: {
          ...this.defaultHeaders,
          ...headers,
        },
        body: data ? JSON.stringify(data) : undefined,
        signal: controller.signal,
      });

      clearTimeout(timeoutId);

      // Parse response headers
      const responseHeaders: Record<string, string> = {};
      response.headers.forEach((value, key) => {
        responseHeaders[key] = value;
      });

      // Parse response data
      let responseData: T;
      const contentType = response.headers.get("content-type");

      if (
        contentType?.includes("application/json") ||
        contentType?.includes("application/fhir+json")
      ) {
        responseData = await response.json();
      } else {
        responseData = (await response.text()) as unknown as T;
      }

      const result: HttpResponse<T> = {
        data: responseData,
        status: response.status,
        statusText: response.statusText,
        headers: responseHeaders,
        config,
      };

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
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

  // FHIR REST API methods
  async read<T extends FhirResource = FhirResource>(resourceType: string, id: string): Promise<T> {
    const response = await this.request<T>({
      method: "GET",
      url: `/${resourceType}/${id}`,
    });
    return response.data;
  }

  async search<_T extends FhirResource = FhirResource>(
    resourceType: string,
    params: Record<string, string | number> = {}
  ): Promise<FhirBundle> {
    const searchParams = new URLSearchParams();

    Object.entries(params).forEach(([key, value]) => {
      searchParams.set(key, String(value));
    });

    const queryString = searchParams.toString();
    const url = `/${resourceType}${queryString ? `?${queryString}` : ""}`;

    const response = await this.request<FhirBundle>({
      method: "GET",
      url,
    });

    return response.data;
  }

  async create<T extends FhirResource = FhirResource>(resource: T): Promise<T> {
    const response = await this.request<T>({
      method: "POST",
      url: `/${resource.resourceType}`,
      data: resource,
    });
    return response.data;
  }

  async update<T extends FhirResource = FhirResource>(resource: T): Promise<T> {
    if (!resource.id) {
      throw new Error("Resource must have an ID for update");
    }

    const response = await this.request<T>({
      method: "PUT",
      url: `/${resource.resourceType}/${resource.id}`,
      data: resource,
    });
    return response.data;
  }

  async delete(resourceType: string, id: string): Promise<void> {
    await this.request({
      method: "DELETE",
      url: `/${resourceType}/${id}`,
    });
  }

  async getCapabilities(): Promise<FhirResource> {
    const response = await this.request<FhirResource>({
      method: "GET",
      url: "/metadata",
    });
    return response.data;
  }

  // Generic request method for custom operations
  async customRequest<T = any>(
    config: Omit<HttpRequestConfig, "url"> & { url: string }
  ): Promise<HttpResponse<T>> {
    return this.request<T>(config);
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

    const response = await this.request<FhirBundle>({
      method: "GET",
      url: link.url,
    });

    return response.data;
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

// Default instance
export const fhirClient = new FhirClient();
