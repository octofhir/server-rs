import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { serverApi } from "../serverApi";
import type { OperationUpdateRequest, SqlValue } from "../types";

// Query keys for cache management
export const systemKeys = {
	all: ["system"] as const,
	health: () => [...systemKeys.all, "health"] as const,
	buildInfo: () => [...systemKeys.all, "buildInfo"] as const,
	settings: () => [...systemKeys.all, "settings"] as const,
	resourceTypes: () => [...systemKeys.all, "resourceTypes"] as const,
	resourceTypesCategorized: () => [...systemKeys.all, "resourceTypesCategorized"] as const,
	jsonSchema: (resourceType: string) => [...systemKeys.all, "jsonSchema", resourceType] as const,
	operations: () => [...systemKeys.all, "operations"] as const,
	operationsFiltered: (filters?: { category?: string; module?: string; public?: boolean }) =>
		[...systemKeys.operations(), filters] as const,
	operation: (id: string) => [...systemKeys.operations(), id] as const,
};

/**
 * Hook to fetch server health status.
 * Polls every 30 seconds by default.
 */
export function useHealth(options?: { refetchInterval?: number | false }) {
	return useQuery({
		queryKey: systemKeys.health(),
		queryFn: () => serverApi.getHealth(),
		refetchInterval: options?.refetchInterval ?? 30000,
		staleTime: 10000,
	});
}

/**
 * Hook to fetch server build information.
 * This data is static, so we cache it for a long time.
 */
export function useBuildInfo() {
	return useQuery({
		queryKey: systemKeys.buildInfo(),
		queryFn: () => serverApi.getBuildInfo(),
		staleTime: 1000 * 60 * 60, // 1 hour
		gcTime: 1000 * 60 * 60 * 24, // 24 hours
	});
}

/**
 * Hook to fetch server settings and feature flags.
 * This data changes rarely, so we cache it for a long time.
 */
export function useSettings() {
	return useQuery({
		queryKey: systemKeys.settings(),
		queryFn: () => serverApi.getSettings(),
		staleTime: 1000 * 60 * 30, // 30 minutes
		gcTime: 1000 * 60 * 60, // 1 hour
	});
}

/**
 * Hook to fetch available FHIR resource types.
 * This data changes rarely, so we cache it for a long time.
 */
export function useResourceTypes() {
	return useQuery({
		queryKey: systemKeys.resourceTypes(),
		queryFn: () => serverApi.getResourceTypes(),
		staleTime: 1000 * 60 * 30, // 30 minutes
	});
}

/**
 * Hook to fetch resource types with category information.
 * Categories: fhir, system, custom
 */
export function useResourceTypesCategorized() {
	return useQuery({
		queryKey: systemKeys.resourceTypesCategorized(),
		queryFn: () => serverApi.getResourceTypesCategorized(),
		staleTime: 1000 * 60 * 30, // 30 minutes
	});
}

/**
 * Hook to fetch JSON Schema for a FHIR resource type.
 * Used for Monaco editor autocomplete and validation.
 */
export function useJsonSchema(resourceType: string | undefined) {
	return useQuery({
		queryKey: systemKeys.jsonSchema(resourceType ?? ""),
		queryFn: async () => {
			if (!resourceType) {
				throw new Error("resourceType is required");
			}
			return serverApi.getJsonSchema(resourceType);
		},
		enabled: !!resourceType,
		staleTime: 1000 * 60 * 60, // 1 hour - schemas are stable
		gcTime: 1000 * 60 * 60 * 24, // 24 hours
	});
}

/**
 * Hook for SQL query execution.
 * Returns a mutation that can be triggered on demand.
 */
export function useSqlMutation() {
	return useMutation({
		mutationFn: ({ query, params }: { query: string; params?: SqlValue[] }) =>
			serverApi.executeSql(query, params),
	});
}

/**
 * Hook for GraphQL query execution.
 * Returns a mutation that can be triggered on demand.
 */
export function useGraphQLMutation() {
	return useMutation({
		mutationFn: ({
			query,
			variables,
			operationName,
		}: {
			query: string;
			variables?: Record<string, unknown>;
			operationName?: string;
		}) => serverApi.executeGraphQL(query, variables, operationName),
	});
}

/**
 * Hook to fetch GraphQL schema.
 * Useful for autocomplete and documentation.
 */
export function useGraphQLSchema() {
	return useQuery({
		queryKey: ["graphql", "schema"] as const,
		queryFn: () => serverApi.getGraphQLSchema(),
		staleTime: 1000 * 60 * 30, // 30 minutes
	});
}

/**
 * Hook to fetch server operations.
 * Operations represent discrete API endpoints that can be targeted by access policies.
 */
export function useOperations(filters?: { category?: string; module?: string; public?: boolean }) {
	return useQuery({
		queryKey: systemKeys.operationsFiltered(filters),
		queryFn: () => serverApi.getOperations(filters),
		staleTime: 1000 * 60 * 5, // 5 minutes
	});
}

/**
 * Hook to fetch a single operation by ID.
 */
export function useOperation(id: string) {
	return useQuery({
		queryKey: systemKeys.operation(id),
		queryFn: () => serverApi.getOperation(id),
		enabled: !!id,
		staleTime: 1000 * 60 * 5, // 5 minutes
	});
}

/**
 * Hook to update an operation.
 * Invalidates the operations cache on success.
 */
export function useUpdateOperation() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: ({ id, update }: { id: string; update: OperationUpdateRequest }) =>
			serverApi.updateOperation(id, update),
		onSuccess: (_data, variables) => {
			// Invalidate the specific operation and all operations lists
			queryClient.invalidateQueries({ queryKey: systemKeys.operation(variables.id) });
			queryClient.invalidateQueries({ queryKey: systemKeys.operations() });
		},
	});
}
