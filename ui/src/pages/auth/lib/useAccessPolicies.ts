import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { notifications } from "@octofhir/ui-kit";
import {
	accessPolicyOperations,
	accessPolicyUserTypes,
	type AccessPolicyResource,
} from "@/entities/access-policy";
import { fhirClient } from "@/shared/api/fhirClient";

export type { AccessPolicyResource, MatcherElement, EngineElement } from "@/entities/access-policy";
export const VALID_OPERATIONS = accessPolicyOperations;
export const VALID_USER_TYPES = accessPolicyUserTypes;

// Query keys
export const accessPolicyKeys = {
	all: ["access-policies"] as const,
	lists: () => [...accessPolicyKeys.all, "list"] as const,
	list: (params: Record<string, unknown>) => [...accessPolicyKeys.lists(), params] as const,
	details: () => [...accessPolicyKeys.all, "detail"] as const,
	detail: (id: string) => [...accessPolicyKeys.details(), id] as const,
};

// Hooks
export function useAccessPolicies(params: { count?: number; offset?: number; search?: string } = {}) {
	return useQuery({
		queryKey: accessPolicyKeys.list(params),
		queryFn: async () => {
			const searchParams: Record<string, string | number> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.name = params.search;

			return fhirClient.search<AccessPolicyResource>("AccessPolicy", searchParams);
		},
	});
}

export function useAccessPolicy(id: string | null) {
	return useQuery({
		queryKey: accessPolicyKeys.detail(id || ""),
		queryFn: async () => {
			if (!id) throw new Error("ID required");
			return fhirClient.read<AccessPolicyResource>("AccessPolicy", id);
		},
		enabled: !!id,
	});
}

export function useCreateAccessPolicy() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: (policy: Partial<AccessPolicyResource>) =>
			fhirClient.create<AccessPolicyResource>({ ...policy, resourceType: "AccessPolicy" }),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: accessPolicyKeys.lists() });
			notifications.show({
				title: "Policy created",
				message: "The access policy has been successfully created.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to create policy",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useUpdateAccessPolicy() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (policy: AccessPolicyResource) => {
			if (!policy.id) throw new Error("Policy resource ID required for update");
			return fhirClient.update<AccessPolicyResource>(policy);
		},
		onSuccess: (data) => {
			queryClient.invalidateQueries({ queryKey: accessPolicyKeys.lists() });
			queryClient.invalidateQueries({ queryKey: accessPolicyKeys.detail(data.id || "") });
			notifications.show({
				title: "Policy updated",
				message: "The access policy has been successfully updated.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to update policy",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useDeleteAccessPolicy() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (id: string) => {
			await fhirClient.delete("AccessPolicy", id);
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: accessPolicyKeys.lists() });
			notifications.show({
				title: "Policy deleted",
				message: "The access policy has been successfully deleted.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to delete policy",
				message: error.message,
				color: "red",
			});
		},
	});
}
