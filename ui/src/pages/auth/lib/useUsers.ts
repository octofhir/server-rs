import { notifications } from "@mantine/notifications";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

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
	list: (params: UserFilterParams) => [...userKeys.lists(), params] as const,
	details: () => [...userKeys.all, "detail"] as const,
	detail: (id: string) => [...userKeys.details(), id] as const,
	sessions: (userId: string) => [...userKeys.all, "sessions", userId] as const,
};

// Hooks
export function useUsers(params: UserFilterParams = {}) {
	return useQuery({
		queryKey: userKeys.list(params),
		queryFn: async () => {
			const searchParams: Record<string, string | number> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.username = params.search;
			if (params.role) searchParams.role = params.role;
			if (params.status) searchParams.status = params.status;
			if (params.active !== undefined) searchParams.active = params.active.toString();

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

// Password reset mutation
export function useResetPassword() {
	return useMutation({
		mutationFn: async ({ userId, newPassword }: { userId: string; newPassword: string }) => {
			const response = await fetch(`/User/${userId}/$reset-password`, {
				method: "POST",
				credentials: "include",
				headers: {
					"Content-Type": "application/json",
					Accept: "application/json",
				},
				body: JSON.stringify({ newPassword }),
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

// User sessions - uses FHIR search on AuthSession resource
export function useUserSessions(userId: string | null) {
	return useQuery({
		queryKey: userKeys.sessions(userId || ""),
		queryFn: async () => {
			if (!userId) throw new Error("User ID required");
			const response = await fhirClient.search("AuthSession", {
				subject: `User/${userId}`,
				status: "active",
			});

			// Transform AuthSession resources to UserSession format
			const sessions: UserSession[] = (response.entry || []).map((entry) => {
				const resource = entry.resource;
				return {
					id: resource.id || "",
					userId: resource.subject?.reference?.replace("User/", "") || userId,
					clientId: resource.client?.reference?.replace("Client/", ""),
					clientName: resource.client?.display,
					ipAddress: resource.ipAddress,
					userAgent: resource.userAgent,
					createdAt: resource.meta?.lastUpdated || resource.createdAt || "",
					expiresAt: resource.expiresAt || "",
					lastActivity: resource.lastActivity,
				};
			});

			return sessions;
		},
		enabled: !!userId,
	});
}

export function useRevokeSession() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async ({ userId, sessionId }: { userId: string; sessionId: string }) => {
			await fhirClient.delete("AuthSession", sessionId);
			return { userId };
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

// Search for FHIR resources (Practitioner or Patient) by name
export function useSearchResources(resourceType: "Practitioner" | "Patient", search: string) {
	return useQuery({
		queryKey: ["resources", resourceType, search],
		queryFn: async () => {
			if (!search || search.length < 2) {
				return { entry: [] };
			}
			const response = await fhirClient.search(resourceType, {
				name: search,
				_count: 10,
			});
			return response as Bundle;
		},
		enabled: search.length >= 2,
	});
}
