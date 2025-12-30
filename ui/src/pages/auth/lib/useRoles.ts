import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { notifications } from "@mantine/notifications";
import { fhirClient } from "@/shared/api/fhirClient";
import type { RoleResource, Bundle, Permission } from "@/shared/api/types";

// Query keys
export const roleKeys = {
	all: ["roles"] as const,
	lists: () => [...roleKeys.all, "list"] as const,
	list: (params: Record<string, unknown>) => [...roleKeys.lists(), params] as const,
	details: () => [...roleKeys.all, "detail"] as const,
	detail: (id: string) => [...roleKeys.details(), id] as const,
	permissions: () => [...roleKeys.all, "permissions"] as const,
};

// Default permissions list (can be fetched from server in the future)
export const DEFAULT_PERMISSIONS: Permission[] = [
	// Resource permissions
	{ code: "read:*", display: "Read All Resources", category: "Resources", description: "Read any FHIR resource" },
	{ code: "write:*", display: "Write All Resources", category: "Resources", description: "Create/update any FHIR resource" },
	{ code: "delete:*", display: "Delete All Resources", category: "Resources", description: "Delete any FHIR resource" },
	{ code: "read:Patient", display: "Read Patients", category: "Resources", description: "Read Patient resources" },
	{ code: "write:Patient", display: "Write Patients", category: "Resources", description: "Create/update Patient resources" },
	{ code: "read:Observation", display: "Read Observations", category: "Resources", description: "Read Observation resources" },
	{ code: "write:Observation", display: "Write Observations", category: "Resources", description: "Create/update Observation resources" },
	// Admin permissions
	{ code: "admin:users", display: "Manage Users", category: "Admin", description: "Create, update, delete users" },
	{ code: "admin:roles", display: "Manage Roles", category: "Admin", description: "Create, update, delete roles" },
	{ code: "admin:clients", display: "Manage Clients", category: "Admin", description: "Create, update, delete OAuth clients" },
	{ code: "admin:policies", display: "Manage Policies", category: "Admin", description: "Create, update, delete access policies" },
	{ code: "admin:packages", display: "Manage Packages", category: "Admin", description: "Install and remove FHIR packages" },
	{ code: "admin:settings", display: "Manage Settings", category: "Admin", description: "View and modify server settings" },
	// System permissions
	{ code: "system:sql", display: "Execute SQL", category: "System", description: "Execute SQL queries in DB console" },
	{ code: "system:graphql", display: "Execute GraphQL", category: "System", description: "Execute GraphQL queries" },
	{ code: "system:operations", display: "Execute Operations", category: "System", description: "Execute FHIR operations" },
];

// Hooks
export function useRoles(params: { count?: number; offset?: number; search?: string } = {}) {
	return useQuery({
		queryKey: roleKeys.list(params),
		queryFn: async () => {
			const searchParams: Record<string, unknown> = {};
			if (params.count) searchParams._count = params.count;
			if (params.offset) searchParams._offset = params.offset;
			if (params.search) searchParams.name = params.search;

			const response = await fhirClient.search("Role", searchParams);
			return response as Bundle<RoleResource>;
		},
	});
}

export function useRole(id: string | null) {
	return useQuery({
		queryKey: roleKeys.detail(id || ""),
		queryFn: async () => {
			if (!id) throw new Error("ID required");
			const response = await fhirClient.read("Role", id);
			return response as RoleResource;
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
		mutationFn: async (role: Partial<RoleResource>) => {
			const response = await fhirClient.create({ ...role, resourceType: "Role" } as RoleResource);
			return response as RoleResource;
		},
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
			const response = await fhirClient.update(role);
			return response as RoleResource;
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
