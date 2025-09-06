import type { BuildInfo, HealthResponse, HttpResponse } from "./types";

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
        headers: {
          "Content-Type": "application/json",
          ...options.headers,
        },
      });

      clearTimeout(timeoutId);

      // Parse response headers
      const headers: Record<string, string> = {};
      response.headers.forEach((value, key) => {
        headers[key] = value;
      });

      // Parse response data
      let data: T;
      const contentType = response.headers.get("content-type");

      if (contentType?.includes("application/json")) {
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
