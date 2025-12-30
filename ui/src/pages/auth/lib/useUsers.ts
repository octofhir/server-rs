import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { notifications } from "@mantine/notifications";
import { fhirClient } from "@/shared/api/fhirClient";
import type { Bundle, UserResource, UserSession } from "@/shared/api/types";

// Filter parameters for user list
export interface UserFilterParams {
	count?: number;
	offset?: number;
	search?: string;
	role?: string;
	status?: "active" | "inactive" | "locked";
	active?: boolean;
}

// Query keys
export const userKeys = {
	all: ["users"] as const,
	lists: () => [...userKeys.all, "list"] as const,
	list: (params: Record<string, unknown>) => [...userKeys.lists(), params] as const,
	details: () => [...userKeys.all, "detail"] as const,
	detail: (id: string) => [...userKeys.details(), id] as const,
	sessions: (userId: string) => [...userKeys.all, "sessions", userId] as const,
};

// Hooks
export function useUsers(params: UserFilterParams = {}) {
	return useQuery({
		queryKey: userKeys.list(params),
		queryFn: async () => {
			const searchParams: Record<string, unknown> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.username = params.search;
			if (params.role) searchParams.role = params.role;
			if (params.status) searchParams.status = params.status;
			if (params.active !== undefined) searchParams.active = params.active;

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
			const response = await fhirClient.create({ ...user, resourceType: "User" } as UserResource);
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
			const response = await fhirClient.update(user);
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

// Password reset mutation (still uses admin endpoint for security)
export function useResetPassword() {
	return useMutation({
		mutationFn: async ({ userId, newPassword }: { userId: string; newPassword: string }) => {
			const response = await fetch(`/admin/users/${userId}/reset-password`, {
				method: "POST",
				credentials: "include",
				headers: {
					"Content-Type": "application/json",
					Accept: "application/json",
				},
				body: JSON.stringify({ password: newPassword }),
			});

			if (!response.ok) {
				const error = await response.json().catch(() => ({ message: response.statusText }));
				throw new Error(error.message || `HTTP ${response.status}`);
			}

			return response.json();
		},
		onSuccess: () => {
			notifications.show({
				title: "Password reset",
				message: "The user's password has been successfully reset.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to reset password",
				message: error.message,
				color: "red",
			});
		},
	});
}

// User sessions (admin endpoints)
export function useUserSessions(userId: string | null) {
	return useQuery({
		queryKey: userKeys.sessions(userId || ""),
		queryFn: async () => {
			if (!userId) throw new Error("User ID required");
			const response = await fetch(`/admin/users/${userId}/sessions`, {
				credentials: "include",
				headers: {
					Accept: "application/json",
				},
			});

			if (!response.ok) {
				const error = await response.json().catch(() => ({ message: response.statusText }));
				throw new Error(error.message || `HTTP ${response.status}`);
			}

			return response.json() as Promise<UserSession[]>;
		},
		enabled: !!userId,
	});
}

export function useRevokeSession() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async ({ userId, sessionId }: { userId: string; sessionId: string }) => {
			const response = await fetch(`/admin/users/${userId}/sessions/${sessionId}`, {
				method: "DELETE",
				credentials: "include",
				headers: {
					Accept: "application/json",
				},
			});

			if (!response.ok) {
				const error = await response.json().catch(() => ({ message: response.statusText }));
				throw new Error(error.message || `HTTP ${response.status}`);
			}
		},
		onSuccess: (_, variables) => {
			queryClient.invalidateQueries({ queryKey: userKeys.sessions(variables.userId) });
			notifications.show({
				title: "Session revoked",
				message: "The session has been successfully revoked.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to revoke session",
				message: error.message,
				color: "red",
			});
		},
	});
}

// Bulk actions
export function useBulkUpdateUsers() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async ({ userIds, updates }: { userIds: string[]; updates: Partial<UserResource> }) => {
			const promises = userIds.map(async (id) => {
				// User is an internal resource, so it's at root level
				const user = await fhirClient.read("User", id);
				const updatedUser = { ...user, ...updates };
				return fhirClient.update(updatedUser);
			});
			return Promise.all(promises);
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: userKeys.lists() });
			notifications.show({
				title: "Users updated",
				message: "The selected users have been successfully updated.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to update users",
				message: error.message,
				color: "red",
			});
		},
	});
}
