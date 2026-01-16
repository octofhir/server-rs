import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { automationsApi } from "@/shared/api/automationsApi";
import type {
  AutomationSearchParams,
  CreateAutomationRequest,
  UpdateAutomationRequest,
  CreateTriggerRequest,
  ExecuteAutomationRequest,
  TestAutomationRequest,
} from "@/shared/api/types";

// Query key factory for automations
export const automationKeys = {
  all: ["automations"] as const,
  lists: () => [...automationKeys.all, "list"] as const,
  list: (filters: AutomationSearchParams) => [...automationKeys.lists(), filters] as const,
  details: () => [...automationKeys.all, "detail"] as const,
  detail: (id: string) => [...automationKeys.details(), id] as const,
  logs: (id: string) => [...automationKeys.all, "logs", id] as const,
};

/**
 * Fetch list of automations with optional filters
 */
export function useAutomations(filters?: AutomationSearchParams) {
  return useQuery({
    queryKey: automationKeys.list(filters ?? {}),
    queryFn: () => automationsApi.list(filters),
  });
}

/**
 * Fetch a single automation by ID (includes triggers)
 */
export function useAutomation(id: string | undefined) {
  return useQuery({
    queryKey: automationKeys.detail(id ?? ""),
    queryFn: () => automationsApi.get(id!),
    enabled: !!id,
  });
}

/**
 * Create a new automation
 */
export function useCreateAutomation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: CreateAutomationRequest) => automationsApi.create(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: automationKeys.lists() });
    },
  });
}

/**
 * Update an existing automation
 */
export function useUpdateAutomation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateAutomationRequest }) =>
      automationsApi.update(id, data),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: automationKeys.detail(variables.id) });
      queryClient.invalidateQueries({ queryKey: automationKeys.lists() });
    },
  });
}

/**
 * Delete an automation
 */
export function useDeleteAutomation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => automationsApi.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: automationKeys.lists() });
    },
  });
}

/**
 * Deploy an automation (transpile and activate)
 */
export function useDeployAutomation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => automationsApi.deploy(id),
    onSuccess: (_data, id) => {
      queryClient.invalidateQueries({ queryKey: automationKeys.detail(id) });
      queryClient.invalidateQueries({ queryKey: automationKeys.lists() });
    },
  });
}

/**
 * Execute an automation manually
 */
export function useExecuteAutomation(automationId: string) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: ExecuteAutomationRequest) => automationsApi.execute(automationId, input),
    onSuccess: () => {
      // Refresh logs after execution
      queryClient.invalidateQueries({ queryKey: automationKeys.logs(automationId) });
    },
  });
}

/**
 * Test automation code without saving (uses code from request body)
 */
export function useTestAutomation(automationId?: string) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: TestAutomationRequest) => automationsApi.test(input),
    onSuccess: () => {
      // Optionally refresh logs if we have an automation ID
      if (automationId) {
        queryClient.invalidateQueries({ queryKey: automationKeys.logs(automationId) });
      }
    },
  });
}

/**
 * Fetch execution logs for an automation
 */
export function useAutomationLogs(id: string | undefined, limit = 50) {
  return useQuery({
    queryKey: automationKeys.logs(id ?? ""),
    queryFn: () => automationsApi.getLogs(id!, limit),
    enabled: !!id,
    refetchInterval: 10000, // Poll every 10 seconds for new executions
  });
}

/**
 * Add a trigger to an automation
 */
export function useAddTrigger() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      automationId,
      trigger,
    }: {
      automationId: string;
      trigger: CreateTriggerRequest;
    }) => automationsApi.addTrigger(automationId, trigger),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: automationKeys.detail(variables.automationId) });
    },
  });
}

/**
 * Delete a trigger from an automation
 */
export function useDeleteTrigger() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ automationId, triggerId }: { automationId: string; triggerId: string }) =>
      automationsApi.deleteTrigger(automationId, triggerId),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: automationKeys.detail(variables.automationId) });
    },
  });
}
