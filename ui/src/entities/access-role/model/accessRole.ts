import type { Permission, RoleResource } from "@/shared/api/types";

export interface RoleTypeView {
	label: string;
	color: string;
	variant: "filled" | "light";
}

export interface RoleStatusView {
	label: string;
	color: string;
}

export const defaultRolePermissions: Permission[] = [
	{ code: "read:*", display: "Read All Resources", category: "Resources", description: "Read any FHIR resource" },
	{ code: "write:*", display: "Write All Resources", category: "Resources", description: "Create/update any FHIR resource" },
	{ code: "delete:*", display: "Delete All Resources", category: "Resources", description: "Delete any FHIR resource" },
	{ code: "read:Patient", display: "Read Patients", category: "Resources", description: "Read Patient resources" },
	{ code: "write:Patient", display: "Write Patients", category: "Resources", description: "Create/update Patient resources" },
	{ code: "read:Observation", display: "Read Observations", category: "Resources", description: "Read Observation resources" },
	{ code: "write:Observation", display: "Write Observations", category: "Resources", description: "Create/update Observation resources" },
	{ code: "admin:users", display: "Manage Users", category: "Admin", description: "Create, update, delete users" },
	{ code: "admin:roles", display: "Manage Roles", category: "Admin", description: "Create, update, delete roles" },
	{ code: "admin:clients", display: "Manage Clients", category: "Admin", description: "Create, update, delete OAuth clients" },
	{ code: "admin:policies", display: "Manage Policies", category: "Admin", description: "Create, update, delete access policies" },
	{ code: "admin:packages", display: "Manage Packages", category: "Admin", description: "Install and remove FHIR packages" },
	{ code: "admin:settings", display: "Manage Settings", category: "Admin", description: "View and modify server settings" },
	{ code: "system:sql", display: "Execute SQL", category: "System", description: "Execute SQL queries in DB console" },
	{ code: "system:graphql", display: "Execute GraphQL", category: "System", description: "Execute GraphQL queries" },
	{ code: "system:operations", display: "Execute Operations", category: "System", description: "Execute FHIR operations" },
];

export function getRoleTypeView(role: RoleResource): RoleTypeView {
	return role.isSystem
		? { label: "System", color: "gray", variant: "filled" }
		: { label: "Custom", color: "blue", variant: "light" };
}

export function getRoleStatusView(role: RoleResource): RoleStatusView {
	return role.active
		? { label: "Active", color: "green" }
		: { label: "Inactive", color: "gray" };
}

export function getRolePermissionPreview(role: RoleResource, limit = 3): {
	visible: string[];
	remaining: number;
} {
	const permissions = role.permissions ?? [];
	return {
		visible: permissions.slice(0, limit),
		remaining: Math.max(permissions.length - limit, 0),
	};
}

export function groupRolePermissions(permissions: Permission[]): Record<string, Permission[]> {
	return permissions.reduce((groups, permission) => {
		groups[permission.category] ??= [];
		groups[permission.category].push(permission);
		return groups;
	}, {} as Record<string, Permission[]>);
}

