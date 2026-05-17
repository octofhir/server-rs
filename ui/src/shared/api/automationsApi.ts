import type {
  Automation,
  AutomationExecution,
  AutomationExecutionStats,
  AutomationListResponse,
  AutomationLogEntry,
  AutomationSearchParams,
  AutomationStatus,
  AutomationTrigger,
  AutomationTriggerType,
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
import { isRecord } from "./guards";

export class AutomationFeatureUnavailableError extends Error {
  constructor(message = "Automations are disabled on this server") {
    super(message);
    this.name = "AutomationFeatureUnavailableError";
  }
}

export function isAutomationFeatureUnavailableError(error: unknown): error is AutomationFeatureUnavailableError {
  return error instanceof AutomationFeatureUnavailableError;
}

const AUTOMATION_STATUSES: readonly string[] = ["active", "inactive", "error"];
const TRIGGER_TYPES: readonly string[] = ["resource_event", "cron", "manual"];
const EXECUTION_STATUSES: readonly string[] = ["running", "completed", "failed"];
const LOG_LEVELS: readonly string[] = ["log", "info", "debug", "warn", "error"];

function isAutomationStatus(value: unknown): value is AutomationStatus {
  return typeof value === "string" && AUTOMATION_STATUSES.includes(value);
}

function isTriggerType(value: unknown): value is AutomationTriggerType {
  return typeof value === "string" && TRIGGER_TYPES.includes(value);
}

function isExecutionStatus(value: unknown): value is AutomationExecution["status"] {
  return typeof value === "string" && EXECUTION_STATUSES.includes(value);
}

function isLogLevel(value: unknown): value is AutomationLogEntry["level"] {
  return typeof value === "string" && LOG_LEVELS.includes(value);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function isExecutionStats(value: unknown): value is AutomationExecutionStats {
  return (
    isRecord(value) &&
    typeof value.failure_count_24h === "number" &&
    typeof value.success_count_24h === "number" &&
    (value.last_execution_status === undefined ||
      isExecutionStatus(value.last_execution_status)) &&
    (value.last_execution_at === undefined || typeof value.last_execution_at === "string") &&
    (value.last_error === undefined || typeof value.last_error === "string")
  );
}

function isAutomationTrigger(value: unknown): value is AutomationTrigger {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.automation_id === "string" &&
    isTriggerType(value.trigger_type) &&
    (value.resource_type === undefined || typeof value.resource_type === "string") &&
    (value.event_types === undefined || isStringArray(value.event_types)) &&
    (value.fhirpath_filter === undefined || typeof value.fhirpath_filter === "string") &&
    (value.cron_expression === undefined || typeof value.cron_expression === "string") &&
    typeof value.created_at === "string"
  );
}

function isAutomation(value: unknown): value is Automation {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.name === "string" &&
    (value.description === undefined || typeof value.description === "string") &&
    typeof value.source_code === "string" &&
    (value.compiled_code === undefined || typeof value.compiled_code === "string") &&
    isAutomationStatus(value.status) &&
    typeof value.version === "number" &&
    typeof value.timeout_ms === "number" &&
    typeof value.created_at === "string" &&
    typeof value.updated_at === "string" &&
    (value.triggers === undefined ||
      (Array.isArray(value.triggers) && value.triggers.every(isAutomationTrigger))) &&
    (value.execution_stats === undefined || isExecutionStats(value.execution_stats))
  );
}

function isAutomationLogEntry(value: unknown): value is AutomationLogEntry {
  return (
    isRecord(value) &&
    isLogLevel(value.level) &&
    typeof value.message === "string" &&
    (value.timestamp === undefined || typeof value.timestamp === "string")
  );
}

function isAutomationExecution(value: unknown): value is AutomationExecution {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.automation_id === "string" &&
    (value.trigger_id === undefined || typeof value.trigger_id === "string") &&
    isExecutionStatus(value.status) &&
    (value.logs === undefined ||
      (Array.isArray(value.logs) && value.logs.every(isAutomationLogEntry))) &&
    (value.error === undefined || typeof value.error === "string") &&
    typeof value.started_at === "string" &&
    (value.completed_at === undefined || typeof value.completed_at === "string") &&
    (value.duration_ms === undefined || typeof value.duration_ms === "number")
  );
}

function isExecuteAutomationResponse(value: unknown): value is ExecuteAutomationResponse {
  return (
    isRecord(value) &&
    typeof value.execution_id === "string" &&
    typeof value.success === "boolean" &&
    (value.logs === undefined ||
      (Array.isArray(value.logs) && value.logs.every(isAutomationLogEntry))) &&
    (value.error === undefined || typeof value.error === "string") &&
    (value.duration_ms === undefined || typeof value.duration_ms === "number")
  );
}

function readBundleResources<T>(
  value: unknown,
  guard: (item: unknown) => item is T,
): { resources: T[]; total: number } {
  if (!isRecord(value)) {
    return { resources: [], total: 0 };
  }

  const resources = Array.isArray(value.entry)
    ? value.entry
        .filter(isRecord)
        .map((entry) => entry.resource)
        .filter(guard)
    : [];

  return {
    resources,
    total: typeof value.total === "number" ? value.total : resources.length,
  };
}

function assertResponse<T>(
  value: unknown,
  guard: (item: unknown) => item is T,
  context: string,
): T {
  if (!guard(value)) {
    throw new Error(`${context}: invalid response`);
  }
  return value;
}

function getErrorText(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }

  if (isRecord(value)) {
    for (const key of ["message", "error", "error_description", "details"]) {
      const message = value[key];
      if (typeof message === "string") {
        return message;
      }
    }
  }

  return "";
}

function isUnavailableResponse(status: number, data: unknown): boolean {
  if (status === 404 || status === 501) {
    return true;
  }

  if (status !== 503) {
    return false;
  }

  const message = getErrorText(data).toLowerCase();
  return message.includes("automation") && (message.includes("disabled") || message.includes("unavailable"));
}

class AutomationsApiClient {
  private baseUrl: string;
  private defaultTimeout: number;

  constructor(baseUrl = "", timeout = 30000) {
    this.baseUrl = baseUrl;
    this.defaultTimeout = timeout;
  }

  private async request(endpoint: string, options: RequestInit = {}): Promise<unknown> {
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
    let data: unknown;
    const contentType = response.headers.get("content-type");

    if (response.status === 204) {
      data = undefined;
    } else if (contentType?.includes("application/json")) {
      data = await response.json();
    } else {
      data = await response.text();
    }

    if (!response.ok) {
      if (isUnavailableResponse(response.status, data)) {
        throw new AutomationFeatureUnavailableError();
      }

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
    const bundle = await this.request(url);
    const { resources, total } = readBundleResources(bundle, isAutomation);

    return {
      automations: resources,
      total,
    };
  }

  /**
   * Get a single automation by ID (includes triggers)
   */
  async get(id: string): Promise<Automation> {
    return assertResponse(
      await this.request(`/api/automations/${encodeURIComponent(id)}`),
      isAutomation,
      "get automation",
    );
  }

  /**
   * Create a new automation
   */
  async create(data: CreateAutomationRequest): Promise<Automation> {
    return assertResponse(
      await this.request("/api/automations", {
        method: "POST",
        body: JSON.stringify(data),
      }),
      isAutomation,
      "create automation",
    );
  }

  /**
   * Update an existing automation
   */
  async update(id: string, data: UpdateAutomationRequest): Promise<Automation> {
    return assertResponse(
      await this.request(`/api/automations/${encodeURIComponent(id)}`, {
        method: "PUT",
        body: JSON.stringify(data),
      }),
      isAutomation,
      "update automation",
    );
  }

  /**
   * Delete an automation
   */
  async delete(id: string): Promise<void> {
    await this.request(`/api/automations/${encodeURIComponent(id)}`, {
      method: "DELETE",
    });
  }

  // ============ Deploy & Execute ============

  /**
   * Deploy an automation (transpile TypeScript and activate)
   */
  async deploy(id: string): Promise<Automation> {
    return assertResponse(
      await this.request(`/api/automations/${encodeURIComponent(id)}/deploy`, {
        method: "POST",
      }),
      isAutomation,
      "deploy automation",
    );
  }

  /**
   * Execute an automation manually (for testing)
   */
  async execute(id: string, input: ExecuteAutomationRequest): Promise<ExecuteAutomationResponse> {
    return assertResponse(
      await this.request(`/api/automations/${encodeURIComponent(id)}/execute`, {
        method: "POST",
        body: JSON.stringify(input),
      }),
      isExecuteAutomationResponse,
      "execute automation",
    );
  }

  /**
   * Test automation code without saving (uses code from request body)
   */
  async test(input: TestAutomationRequest): Promise<ExecuteAutomationResponse> {
    return assertResponse(
      await this.request("/api/automations/test", {
        method: "POST",
        body: JSON.stringify(input),
      }),
      isExecuteAutomationResponse,
      "test automation",
    );
  }

  // ============ Execution Logs ============

  /**
   * Get execution history for an automation
   * Backend returns a Bundle, so we extract entries
   */
  async getLogs(id: string, limit = 50): Promise<AutomationExecution[]> {
    const url = `/api/automations/${encodeURIComponent(id)}/logs?limit=${limit}`;
    const bundle = await this.request(url);
    return readBundleResources(bundle, isAutomationExecution).resources;
  }

  // ============ Triggers ============

  /**
   * Add a trigger to an automation
   */
  async addTrigger(automationId: string, trigger: CreateTriggerRequest): Promise<AutomationTrigger> {
    return assertResponse(
      await this.request(`/api/automations/${encodeURIComponent(automationId)}/triggers`, {
        method: "POST",
        body: JSON.stringify(trigger),
      }),
      isAutomationTrigger,
      "add automation trigger",
    );
  }

  /**
   * Delete a trigger from an automation
   */
  async deleteTrigger(automationId: string, triggerId: string): Promise<void> {
    await this.request(
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
