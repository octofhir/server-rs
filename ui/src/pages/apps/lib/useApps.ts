import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { notify } from "@octofhir/ui-kit";
import type { AppResource } from "@/entities/api-app";
import { fhirClient } from "@/shared/api/fhirClient";

export type { AppResource } from "@/entities/api-app";

// Query keys
export const appKeys = {
	all: ["apps"] as const,
	lists: () => [...appKeys.all, "list"] as const,
	list: (params: Record<string, unknown>) => [...appKeys.lists(), params] as const,
	details: () => [...appKeys.all, "detail"] as const,
	detail: (id: string) => [...appKeys.details(), id] as const,
};

// Hooks
export function useApps(params: { count?: number; offset?: number; search?: string } = {}) {
	return useQuery({
		queryKey: appKeys.list(params),
		queryFn: async () => {
			const searchParams: Record<string, string | number> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.name = params.search;
			
			return fhirClient.search<AppResource>("App", searchParams);
		},
	});
}

export function useApp(id: string | null) {
	return useQuery({
		queryKey: appKeys.detail(id || ""),
		queryFn: async () => {
			if (!id) throw new Error("ID required");
			return fhirClient.read<AppResource>("App", id);
		},
		enabled: !!id,
	});
}

export function useCreateApp() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (app: AppResource) => {
			const response = await fhirClient.create(app);
			return response;
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: appKeys.lists() });
			notify({
				theme: "success",
				title: "App created",
				content: "The API application has been successfully created.",
			});
		},
		onError: (error: Error) => {
			notify({
				theme: "danger",
				title: "Failed to create app",
				content: error.message,
			});
		},
	});
}

export function useUpdateApp() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (app: AppResource) => {
			if (!app.id) throw new Error("App resource ID required for update");
			const response = await fhirClient.update(app);
			return response;
		},
		onSuccess: (data) => {
			queryClient.invalidateQueries({ queryKey: appKeys.lists() });
			queryClient.invalidateQueries({ queryKey: appKeys.detail(data.id || "") });
			notify({
				theme: "success",
				title: "App updated",
				content: "The API application has been successfully updated.",
			});
		},
		onError: (error: Error) => {
			notify({
				theme: "danger",
				title: "Failed to update app",
				content: error.message,
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
			notify({
				theme: "success",
				title: "App deleted",
				content: "The API application has been successfully deleted.",
			});
		},
		onError: (error: Error) => {
			notify({
				theme: "danger",
				title: "Failed to delete app",
				content: error.message,
			});
		},
	});
}
