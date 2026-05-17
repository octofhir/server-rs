import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { notifications } from "@octofhir/ui-kit";
import { defaultRolePermissions } from "@/entities/access-role";
import { fhirClient } from "@/shared/api/fhirClient";
import type { RoleResource } from "@/shared/api/types";

// Query keys
export const roleKeys = {
	all: ["roles"] as const,
	lists: () => [...roleKeys.all, "list"] as const,
	list: (params: Record<string, unknown>) => [...roleKeys.lists(), params] as const,
	details: () => [...roleKeys.all, "detail"] as const,
	detail: (id: string) => [...roleKeys.details(), id] as const,
	permissions: () => [...roleKeys.all, "permissions"] as const,
};

export const DEFAULT_PERMISSIONS = defaultRolePermissions;

// Hooks
export function useRoles(params: { count?: number; offset?: number; search?: string } = {}) {
	return useQuery({
		queryKey: roleKeys.list(params),
		queryFn: async () => {
			const searchParams: Record<string, string | number> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.name = params.search;

			return fhirClient.search<RoleResource>("Role", searchParams);
		},
	});
}

export function useRole(id: string | null) {
	return useQuery({
		queryKey: roleKeys.detail(id || ""),
		queryFn: async () => {
			if (!id) throw new Error("ID required");
			return fhirClient.read<RoleResource>("Role", id);
		},
		enabled: !!id,
	});
}

export function usePermissions() {
	return useQuery({
		queryKey: roleKeys.permissions(),
		queryFn: async () => {
			// In the future, this could fetch from /admin/permissions
			// For now, return the default permissions
			return DEFAULT_PERMISSIONS;
		},
		staleTime: Number.POSITIVE_INFINITY, // Permissions rarely change
	});
}

export function useCreateRole() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: (role: Partial<RoleResource>) =>
			fhirClient.create<RoleResource>({ ...role, resourceType: "Role" }),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: roleKeys.lists() });
			notifications.show({
				title: "Role created",
				message: "The role has been successfully created.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to create role",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useUpdateRole() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (role: RoleResource) => {
			if (!role.id) throw new Error("Role ID required for update");
			return fhirClient.update<RoleResource>(role);
		},
		onSuccess: (data) => {
			queryClient.invalidateQueries({ queryKey: roleKeys.lists() });
			queryClient.invalidateQueries({ queryKey: roleKeys.detail(data.id || "") });
			notifications.show({
				title: "Role updated",
				message: "The role has been successfully updated.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to update role",
				message: error.message,
				color: "red",
			});
		},
	});
}

export function useDeleteRole() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: async (id: string) => {
			await fhirClient.delete("Role", id);
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: roleKeys.lists() });
			notifications.show({
				title: "Role deleted",
				message: "The role has been successfully deleted.",
				color: "green",
			});
		},
		onError: (error: Error) => {
			notifications.show({
				title: "Failed to delete role",
				message: error.message,
				color: "red",
			});
		},
	});
}
