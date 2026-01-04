import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { notifications } from "@mantine/notifications";
import { fhirClient } from "@/shared/api/fhirClient";
import type { Bundle, FhirResource } from "@/shared/api/types";

// We'll define a Client interface that matches the backend
export interface ClientResource extends FhirResource {
	resourceType: "Client";
	clientId: string;
	clientSecret?: string;
	name: string;
	description?: string;
	grantTypes: string[];
	redirectUris: string[];
	postLogoutRedirectUris: string[];
	scopes: string[];
	confidential: boolean;
	active: boolean;
	accessTokenLifetime?: number;
	refreshTokenLifetime?: number;
	pkceRequired?: boolean;
	allowedOrigins: string[];
	jwksUri?: string;
}

// Response from regenerate secret endpoint
export interface RegenerateSecretResponse {
	clientId: string;
	clientSecret: string;
}

// Query keys
export const clientKeys = {
	all: ["clients"] as const,
	lists: () => [...clientKeys.all, "list"] as const,
	list: (params: Record<string, any>) => [...clientKeys.lists(), params] as const,
	details: () => [...clientKeys.all, "detail"] as const,
	detail: (id: string) => [...clientKeys.details(), id] as const,
};

// Hooks
export function useClients(params: { count?: number; offset?: number; search?: string } = {}) {
	return useQuery({
		queryKey: clientKeys.list(params),
		queryFn: async () => {
			const searchParams: Record<string, any> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.name = params.search;
			
			const response = await fhirClient.search("Client", searchParams);
			return response as Bundle<ClientResource>;
		},
	});
}

export function useClient(id: string | null) {
	return useQuery({
		queryKey: clientKeys.detail(id || ""),
		queryFn: async () => {
			if (!id) throw new Error("ID required");
			const response = await fhirClient.read("Client", id);
			return response as ClientResource;
		},
		enabled: !!id,
	});
}

export function useCreateClient() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (client: Partial<ClientResource>) => {
			// Strip 'id' from body - server assigns the ID for new resources
			const { id: _id, ...body } = client;
			const response = await fhirClient.create(body as Partial<ClientResource>);
			return response as ClientResource;
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: clientKeys.lists() });
			notifications.show({
				title: "Client created",
				message: "The OAuth client has been successfully created.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to create client",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useUpdateClient() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (client: ClientResource) => {
			if (!client.id) throw new Error("Client resource ID required for update");
			// Use fhirClient.update which handles routing correctly
			const response = await fhirClient.update(client);
			return response as ClientResource;
		},
		onSuccess: (data) => {
			queryClient.invalidateQueries({ queryKey: clientKeys.lists() });
			queryClient.invalidateQueries({ queryKey: clientKeys.detail(data.id || "") });
			notifications.show({
				title: "Client updated",
				message: "The OAuth client has been successfully updated.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to update client",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useDeleteClient() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (id: string) => {
			await fhirClient.delete("Client", id);
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: clientKeys.lists() });
			notifications.show({
				title: "Client deleted",
				message: "The OAuth client has been successfully deleted.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to delete client",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useRegenerateSecret() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (clientId: string): Promise<RegenerateSecretResponse> => {
			const response = await fetch(`/admin/clients/${clientId}/regenerate-secret`, {
				method: "POST",
				credentials: "include",
				headers: {
					"Content-Type": "application/json",
					Accept: "application/json",
				},
			});

			if (!response.ok) {
				const error = await response.json().catch(() => ({ message: response.statusText }));
				throw new Error(error.message || `HTTP ${response.status}`);
			}

			return response.json();
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: clientKeys.lists() });
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to regenerate secret",
				message: error.message,
				color: "red",
			});
		},
	});
}
