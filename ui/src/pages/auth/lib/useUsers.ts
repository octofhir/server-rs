import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { notifications } from "@mantine/notifications";
import { fhirClient } from "@/shared/api/fhirClient";
import type { UserResource, Bundle } from "@/shared/api/types";

// Query keys
export const userKeys = {
	all: ["users"] as const,
	lists: () => [...userKeys.all, "list"] as const,
	list: (params: Record<string, any>) => [...userKeys.lists(), params] as const,
	details: () => [...userKeys.all, "detail"] as const,
	detail: (id: string) => [...userKeys.details(), id] as const,
};

// Hooks
export function useUsers(params: { count?: number; offset?: number; search?: string } = {}) {
	return useQuery({
		queryKey: userKeys.list(params),
		queryFn: async () => {
			const searchParams: Record<string, any> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.username = params.search; // Simple search by username
			
			// We use the fhirClient because /User endpoint returns a Bundle
			// but we need to cast it since fhirClient types are strictly FHIR resources
			const response = await fhirClient.search("User", searchParams);
			return response as Bundle<UserResource>;
		},
	});
}

export function useUser(id: string | null) {
	return useQuery({
		queryKey: userKeys.detail(id || ""),
		queryFn: async () => {
			if (!id) throw new Error("ID required");
			const response = await fhirClient.read("User", id);
			return response as UserResource;
		},
		enabled: !!id,
	});
}

export function useCreateUser() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (user: Partial<UserResource>) => {
			const response = await fhirClient.create(user as any);
			return response as UserResource;
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: userKeys.lists() });
			notifications.show({
				title: "User created",
				message: "The user has been successfully created.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to create user",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useUpdateUser() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (user: UserResource) => {
			if (!user.id) throw new Error("User ID required for update");
			const response = await fhirClient.update(user as any);
			return response as UserResource;
		},
		onSuccess: (data) => {
			queryClient.invalidateQueries({ queryKey: userKeys.lists() });
			queryClient.invalidateQueries({ queryKey: userKeys.detail(data.id || "") });
			notifications.show({
				title: "User updated",
				message: "The user has been successfully updated.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to update user",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useDeleteUser() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (id: string) => {
			await fhirClient.delete("User", id);
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: userKeys.lists() });
			notifications.show({
				title: "User deleted",
				message: "The user has been successfully deleted.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to delete user",
				message: error.message,
				color: "red",
			});
		},
	});
}
