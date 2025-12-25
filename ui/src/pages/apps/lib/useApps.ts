import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { notifications } from "@mantine/notifications";
import { fhirClient } from "@/shared/api/fhirClient";
import type { Bundle, FhirResource } from "@/shared/api/types";

export interface AppResource extends FhirResource {
	resourceType: "App";
	name: string;
	description?: string;
	basePath: string;
	active: boolean;
	authentication?: {
		type: string;
		required: boolean;
	};
}

// Query keys
export const appKeys = {
	all: ["apps"] as const,
	lists: () => [...appKeys.all, "list"] as const,
	list: (params: Record<string, any>) => [...appKeys.lists(), params] as const,
	details: () => [...appKeys.all, "detail"] as const,
	detail: (id: string) => [...appKeys.details(), id] as const,
};

// Hooks
export function useApps(params: { count?: number; offset?: number; search?: string } = {}) {
	return useQuery({
		queryKey: appKeys.list(params),
		queryFn: async () => {
			const searchParams: Record<string, any> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.name = params.search;
			
			const response = await fhirClient.search("App", searchParams);
			return response as Bundle<AppResource>;
		},
	});
}

export function useApp(id: string | null) {
	return useQuery({
		queryKey: appKeys.detail(id || ""),
		queryFn: async () => {
			if (!id) throw new Error("ID required");
			const response = await fhirClient.read("App", id);
			return response as AppResource;
		},
		enabled: !!id,
	});
}

export function useCreateApp() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (app: Partial<AppResource>) => {
			const response = await fhirClient.create(app as any);
			return response as AppResource;
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: appKeys.lists() });
			notifications.show({
				title: "App created",
				message: "The API application has been successfully created.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to create app",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useUpdateApp() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (app: AppResource) => {
			if (!app.id) throw new Error("App resource ID required for update");
			const response = await fhirClient.update(app as any);
			return response as AppResource;
		},
		onSuccess: (data) => {
			queryClient.invalidateQueries({ queryKey: appKeys.lists() });
			queryClient.invalidateQueries({ queryKey: appKeys.detail(data.id || "") });
			notifications.show({
				title: "App updated",
				message: "The API application has been successfully updated.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to update app",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useDeleteApp() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (id: string) => {
			await fhirClient.delete("App", id);
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: appKeys.lists() });
			notifications.show({
				title: "App deleted",
				message: "The API application has been successfully deleted.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to delete app",
				message: error.message,
				color: "red",
			});
		},
	});
}
