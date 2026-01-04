import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { notifications } from "@mantine/notifications";
import { fhirClient } from "@/shared/api/fhirClient";
import type { Bundle, FhirResource } from "@/shared/api/types";

/**
 * AccessPolicy resource matching backend structure.
 *
 * Uses matcher + engine pattern for policy evaluation.
 */
export interface AccessPolicyResource extends FhirResource {
	resourceType: "AccessPolicy";
	name: string;
	description?: string;
	active?: boolean;
	priority?: number;
	matcher?: MatcherElement;
	engine: EngineElement;
	denyMessage?: string;
}

/**
 * Matcher element - determines when a policy applies.
 * All specified fields must match for the policy to apply (AND logic).
 */
export interface MatcherElement {
	/** Client ID patterns (supports wildcards with `*`). */
	clients?: string[];
	/** Required user roles (any role matches). */
	roles?: string[];
	/** User FHIR resource types (e.g., "Practitioner", "Patient"). */
	userTypes?: string[];
	/** Target FHIR resource types. */
	resourceTypes?: string[];
	/** FHIR operations (e.g., "read", "create", "search"). */
	operations?: string[];
	/** Operation IDs for more specific targeting (e.g., "fhir.read", "graphql.query"). */
	operationIds?: string[];
	/** Request path patterns (glob syntax). */
	paths?: string[];
	/** Source IP addresses in CIDR notation. */
	sourceIps?: string[];
}

/**
 * Engine element - how the policy is evaluated.
 */
export interface EngineElement {
	/** Engine type: allow, deny, or quickjs for custom scripts. */
	type: "allow" | "deny" | "quickjs";
	/** Script content (required for QuickJS engine). */
	script?: string;
}

/** Valid FHIR operations for policy matching. */
export const VALID_OPERATIONS = [
	"read",
	"vread",
	"update",
	"patch",
	"delete",
	"history",
	"history-instance",
	"history-type",
	"history-system",
	"create",
	"search",
	"search-type",
	"search-system",
	"capabilities",
	"batch",
	"transaction",
	"operation",
	"*",
] as const;

/** Valid user types for policy matching. */
export const VALID_USER_TYPES = [
	"Practitioner",
	"Patient",
	"RelatedPerson",
	"Person",
	"*",
] as const;

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
			const searchParams: Record<string, unknown> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.name = params.search;

			const response = await fhirClient.search("AccessPolicy", searchParams);
			return response as Bundle<AccessPolicyResource>;
		},
	});
}

export function useAccessPolicy(id: string | null) {
	return useQuery({
		queryKey: accessPolicyKeys.detail(id || ""),
		queryFn: async () => {
			if (!id) throw new Error("ID required");
			const response = await fhirClient.read("AccessPolicy", id);
			return response as AccessPolicyResource;
		},
		enabled: !!id,
	});
}

export function useCreateAccessPolicy() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (policy: Partial<AccessPolicyResource>) => {
			const response = await fhirClient.create(policy as Partial<FhirResource>);
			return response as AccessPolicyResource;
		},
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
			const response = await fhirClient.update(policy as FhirResource);
			return response as AccessPolicyResource;
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
