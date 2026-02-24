import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { notifications } from "@octofhir/ui-kit";
import { fhirClient } from "@/shared/api/fhirClient";
import type { Bundle, FhirResource } from "@/shared/api/types";

export interface IdentityProviderResource extends FhirResource {
	resourceType: "IdentityProvider";
	name: string;
	title?: string;
	description?: string;
	type: "oidc" | "oauth2" | "saml2";
	issuer: string;
	clientId: string;
	clientSecret?: string;
	authorizeUrl: string;
	tokenUrl: string;
	jwksUrl?: string;
	userInfoUrl?: string;
	scopes?: string[];
	active: boolean;
}

// Query keys
export const idpKeys = {
	all: ["identity-providers"] as const,
	lists: () => [...idpKeys.all, "list"] as const,
	list: (params: Record<string, any>) => [...idpKeys.lists(), params] as const,
	details: () => [...idpKeys.all, "detail"] as const,
	detail: (id: string) => [...idpKeys.details(), id] as const,
};

// Hooks
export function useIdentityProviders(params: { count?: number; offset?: number; search?: string } = {}) {
	return useQuery({
		queryKey: idpKeys.list(params),
		queryFn: async () => {
			const searchParams: Record<string, any> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.name = params.search;
			
			const response = await fhirClient.search("IdentityProvider", searchParams);
			return response as Bundle<IdentityProviderResource>;
		},
	});
}

export function useIdentityProvider(id: string | null) {
	return useQuery({
		queryKey: idpKeys.detail(id || ""),
		queryFn: async () => {
			if (!id) throw new Error("ID required");
			const response = await fhirClient.read("IdentityProvider", id);
			return response as IdentityProviderResource;
		},
		enabled: !!id,
	});
}

export function useCreateIdentityProvider() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (idp: Partial<IdentityProviderResource>) => {
			const response = await fhirClient.create(idp as any);
			return response as IdentityProviderResource;
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: idpKeys.lists() });
			notifications.show({
				title: "Provider created",
				message: "The identity provider has been successfully created.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to create provider",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useUpdateIdentityProvider() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (idp: IdentityProviderResource) => {
			if (!idp.id) throw new Error("Provider resource ID required for update");
			const response = await fhirClient.update(idp as any);
			return response as IdentityProviderResource;
		},
		onSuccess: (data) => {
			queryClient.invalidateQueries({ queryKey: idpKeys.lists() });
			queryClient.invalidateQueries({ queryKey: idpKeys.detail(data.id || "") });
			notifications.show({
				title: "Provider updated",
				message: "The identity provider has been successfully updated.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to update provider",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useDeleteIdentityProvider() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (id: string) => {
			await fhirClient.delete("IdentityProvider", id);
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: idpKeys.lists() });
			notifications.show({
				title: "Provider deleted",
				message: "The identity provider has been successfully deleted.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to delete provider",
				message: error.message,
				color: "red",
			});
		},
	});
}
