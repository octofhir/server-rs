import type {
  Automation,
  AutomationExecution,
  AutomationListResponse,
  AutomationSearchParams,
  AutomationTrigger,
  CreateAutomationRequest,
  CreateTriggerRequest,
  ExecuteAutomationRequest,
  ExecuteAutomationResponse,
  TestAutomationRequest,
  UpdateAutomationRequest,
} from "./types";
import { ApiResponseError } from "./serverApi";
import { authInterceptor } from "./authInterceptor";
import { refreshAuthSession } from "./authSession";

class AutomationsApiClient {
  private baseUrl: string;
  private defaultTimeout: number;

  constructor(baseUrl = "", timeout = 30000) {
    this.baseUrl = baseUrl;
    this.defaultTimeout = timeout;
  }

  private async request<T>(endpoint: string, options: RequestInit = {}): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`;
    const executeFetch = async (): Promise<Response> => {
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), this.defaultTimeout);

      try {
        return await fetch(url, {
          ...options,
          signal: controller.signal,
          credentials: "include",
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

    // Parse response
    let data: T;
    const contentType = response.headers.get("content-type");

    if (contentType?.includes("application/json")) {
      data = await response.json();
    } else {
      data = (await response.text()) as unknown as T;
    }

    if (!response.ok) {
      throw new ApiResponseError(
        `HTTP ${response.status}: ${response.statusText}`,
        response.status,
        response.statusText,
        data,
      );
    }

    return data;
  }

  // ============ Automation CRUD ============

  /**
   * List all automations with optional filters
   * Backend returns a Bundle, so we convert to AutomationListResponse
   */
  async list(params?: AutomationSearchParams): Promise<AutomationListResponse> {
    const queryParams = new URLSearchParams();
    if (params?.status) queryParams.set("status", params.status);
    if (params?.name) queryParams.set("name", params.name);
    if (params?._count) queryParams.set("_count", String(params._count));
    if (params?._offset) queryParams.set("_offset", String(params._offset));

    const queryString = queryParams.toString();
    const url = `/api/automations${queryString ? `?${queryString}` : ""}`;

    // Backend returns a Bundle format
    const bundle = await this.request<{
      total?: number;
      entry?: Array<{ resource: Automation }>;
    }>(url);

    return {
      automations: bundle.entry?.map((e) => e.resource) ?? [],
      total: bundle.total ?? 0,
    };
  }

  /**
   * Get a single automation by ID (includes triggers)
   */
  async get(id: string): Promise<Automation> {
    return this.request<Automation>(`/api/automations/${encodeURIComponent(id)}`);
  }

  /**
   * Create a new automation
   */
  async create(data: CreateAutomationRequest): Promise<Automation> {
    return this.request<Automation>("/api/automations", {
      method: "POST",
      body: JSON.stringify(data),
    });
  }

  /**
   * Update an existing automation
   */
  async update(id: string, data: UpdateAutomationRequest): Promise<Automation> {
    return this.request<Automation>(`/api/automations/${encodeURIComponent(id)}`, {
      method: "PUT",
      body: JSON.stringify(data),
    });
  }

  /**
   * Delete an automation
   */
  async delete(id: string): Promise<void> {
    await this.request<void>(`/api/automations/${encodeURIComponent(id)}`, {
      method: "DELETE",
    });
  }

  // ============ Deploy & Execute ============

  /**
   * Deploy an automation (transpile TypeScript and activate)
   */
  async deploy(id: string): Promise<Automation> {
    return this.request<Automation>(`/api/automations/${encodeURIComponent(id)}/deploy`, {
      method: "POST",
    });
  }

  /**
   * Execute an automation manually (for testing)
   */
  async execute(id: string, input: ExecuteAutomationRequest): Promise<ExecuteAutomationResponse> {
    return this.request<ExecuteAutomationResponse>(
      `/api/automations/${encodeURIComponent(id)}/execute`,
      {
        method: "POST",
        body: JSON.stringify(input),
      },
    );
  }

  /**
   * Test automation code without saving (uses code from request body)
   */
  async test(input: TestAutomationRequest): Promise<ExecuteAutomationResponse> {
    return this.request<ExecuteAutomationResponse>("/api/automations/test", {
      method: "POST",
      body: JSON.stringify(input),
    });
  }

  // ============ Execution Logs ============

  /**
   * Get execution history for an automation
   * Backend returns a Bundle, so we extract entries
   */
  async getLogs(id: string, limit = 50): Promise<AutomationExecution[]> {
    const url = `/api/automations/${encodeURIComponent(id)}/logs?limit=${limit}`;
    const bundle = await this.request<{
      entry?: Array<{ resource: AutomationExecution }>;
    }>(url);
    return bundle.entry?.map((e) => e.resource) ?? [];
  }

  // ============ Triggers ============

  /**
   * Add a trigger to an automation
   */
  async addTrigger(automationId: string, trigger: CreateTriggerRequest): Promise<AutomationTrigger> {
    return this.request<AutomationTrigger>(
      `/api/automations/${encodeURIComponent(automationId)}/triggers`,
      {
        method: "POST",
        body: JSON.stringify(trigger),
      },
    );
  }

  /**
   * Delete a trigger from an automation
   */
  async deleteTrigger(automationId: string, triggerId: string): Promise<void> {
    await this.request<void>(
      `/api/automations/${encodeURIComponent(automationId)}/triggers/${encodeURIComponent(triggerId)}`,
      {
        method: "DELETE",
      },
    );
  }
}

// Default instance
export const automationsApi = new AutomationsApiClient();
export { AutomationsApiClient };
