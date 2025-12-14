import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { fhirClient } from "../fhirClient";
import type { FhirResource, FhirBundle } from "../types";

// Query keys for FHIR resources
export const fhirKeys = {
	all: ["fhir"] as const,
	capabilities: () => [...fhirKeys.all, "capabilities"] as const,
	resource: (type: string, id: string) => [...fhirKeys.all, "resource", type, id] as const,
	search: (type: string, params?: Record<string, string | number>) =>
		[...fhirKeys.all, "search", type, params ?? {}] as const,
	searchList: (type: string) => [...fhirKeys.all, "search", type] as const,
};

/**
 * Hook to fetch FHIR capability statement (metadata).
 */
export function useCapabilities() {
	return useQuery({
		queryKey: fhirKeys.capabilities(),
		queryFn: () => fhirClient.getCapabilities(),
		staleTime: 1000 * 60 * 30, // 30 minutes
	});
}

/**
 * Hook to read a single FHIR resource by type and ID.
 */
export function useResource<T extends FhirResource = FhirResource>(
	resourceType: string,
	id: string,
	options?: { enabled?: boolean }
) {
	return useQuery({
		queryKey: fhirKeys.resource(resourceType, id),
		queryFn: () => fhirClient.read<T>(resourceType, id),
		enabled: options?.enabled ?? (!!resourceType && !!id),
	});
}

/**
 * Hook to search FHIR resources.
 */
export function useResourceSearch<T extends FhirResource = FhirResource>(
	resourceType: string,
	params?: Record<string, string | number>,
	options?: { enabled?: boolean }
) {
	return useQuery({
		queryKey: fhirKeys.search(resourceType, params),
		queryFn: () => fhirClient.search<T>(resourceType, params ?? {}),
		enabled: options?.enabled ?? !!resourceType,
	});
}

/**
 * Hook to create a new FHIR resource.
 * Invalidates relevant search queries on success.
 */
export function useCreateResource<T extends FhirResource = FhirResource>() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: (resource: T) => fhirClient.create<T>(resource),
		onSuccess: (data) => {
			// Invalidate search queries for this resource type
			queryClient.invalidateQueries({
				queryKey: fhirKeys.searchList(data.resourceType),
			});
		},
	});
}

/**
 * Hook to update an existing FHIR resource.
 * Invalidates the specific resource and related search queries on success.
 */
export function useUpdateResource<T extends FhirResource = FhirResource>() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: (resource: T) => fhirClient.update<T>(resource),
		onSuccess: (data) => {
			// Update the specific resource in cache
			if (data.id) {
				queryClient.setQueryData(fhirKeys.resource(data.resourceType, data.id), data);
			}
			// Invalidate search queries for this resource type
			queryClient.invalidateQueries({
				queryKey: fhirKeys.searchList(data.resourceType),
			});
		},
	});
}

/**
 * Hook to delete a FHIR resource.
 * Invalidates relevant queries on success.
 */
export function useDeleteResource() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: ({ resourceType, id }: { resourceType: string; id: string }) =>
			fhirClient.delete(resourceType, id),
		onSuccess: (_, { resourceType, id }) => {
			// Remove from cache
			queryClient.removeQueries({
				queryKey: fhirKeys.resource(resourceType, id),
			});
			// Invalidate search queries
			queryClient.invalidateQueries({
				queryKey: fhirKeys.searchList(resourceType),
			});
		},
	});
}

/**
 * Hook to follow a bundle navigation link.
 */
export function useFollowBundleLink() {
	return useMutation({
		mutationFn: ({
			bundle,
			relation,
		}: {
			bundle: FhirBundle;
			relation: "first" | "prev" | "next" | "last";
		}) => fhirClient.followLink(bundle, relation),
	});
}
