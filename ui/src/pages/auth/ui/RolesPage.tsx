import { useState, useEffect, useMemo } from "react";
import {
	Stack,
	Title,
	Text,
	Group,
	Table,
	Badge,
	Menu,
	Checkbox,
	Textarea,
	Alert,
} from "@mantine/core";
import { useDisclosure, useDebouncedValue } from "@mantine/hooks";
import { useForm } from "@mantine/form";
import {
	IconPlus,
	IconSearch,
	IconDotsVertical,
	IconEdit,
	IconTrash,
	IconShield,
	IconAlertTriangle,
} from "@tabler/icons-react";
import { Card } from "@/shared/ui/Card/Card";
import { Modal } from "@/shared/ui/Modal/Modal";
import { Button } from "@/shared/ui/Button/Button";
import { TextInput } from "@/shared/ui/TextInput/TextInput";
import { ActionIcon } from "@/shared/ui/ActionIcon/ActionIcon";
import {
	useRoles,
	useCreateRole,
	useUpdateRole,
	useDeleteRole,
	usePermissions,
	DEFAULT_PERMISSIONS,
} from "../lib/useRoles";
import type { RoleResource, Permission } from "@/shared/api/types";
import classes from "./RolesPage.module.css";

export function RolesPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingRole, setEditingRole] = useState<RoleResource | null>(null);
	const [deleteTarget, setDeleteTarget] = useState<RoleResource | null>(null);

	const { data, isLoading } = useRoles({ search: debouncedSearch || undefined });
	const deleteRole = useDeleteRole();

	const roles = data?.entry?.map((e) => e.resource) || [];

	const handleEdit = (role: RoleResource) => {
		setEditingRole(role);
		open();
	};

	const handleDeleteClick = (role: RoleResource) => {
		setDeleteTarget(role);
	};

	const handleDeleteConfirm = () => {
		if (deleteTarget?.id) {
			deleteRole.mutate(deleteTarget.id, {
				onSuccess: () => setDeleteTarget(null),
			});
		}
	};

	const handleClose = () => {
		setEditingRole(null);
		close();
	};

	return (
		<Stack gap="md" className={classes.pageRoot}>
			<Group justify="space-between">
				<div>
					<Title order={2}>Roles</Title>
					<Text c="dimmed" size="sm">
						Manage roles and permissions
					</Text>
				</div>
				<Button leftSection={<IconPlus size={16} />} onClick={open}>
					Create Role
				</Button>
			</Group>

			<Card className={classes.tableContainer}>
				<Group mb="md">
					<TextInput
						placeholder="Search roles..."
						leftSection={<IconSearch size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						style={{ flex: 1 }}
					/>
				</Group>

				<Table>
					<Table.Thead>
						<Table.Tr>
							<Table.Th>Role</Table.Th>
							<Table.Th>Permissions</Table.Th>
							<Table.Th>Type</Table.Th>
							<Table.Th>Status</Table.Th>
							<Table.Th style={{ width: 50 }} />
						</Table.Tr>
					</Table.Thead>
					<Table.Tbody>
						{isLoading ? (
							<Table.Tr>
								<Table.Td colSpan={5}>Loading...</Table.Td>
							</Table.Tr>
						) : roles.length === 0 ? (
							<Table.Tr>
								<Table.Td colSpan={5} style={{ textAlign: "center" }}>
									No roles found
								</Table.Td>
							</Table.Tr>
						) : (
							roles.map((role) => (
								<Table.Tr key={role.id}>
									<Table.Td>
										<div className={classes.roleInfo}>
											<div className={classes.roleIcon}>
												<IconShield size={18} />
											</div>
											<div>
												<Text className={classes.roleName}>{role.name}</Text>
												<Text className={classes.roleDescription}>
													{role.description || "No description"}
												</Text>
											</div>
										</div>
									</Table.Td>
									<Table.Td>
										<Group gap={4}>
											{role.permissions?.slice(0, 3).map((perm) => (
												<Badge key={perm} size="sm" variant="dot">
													{perm}
												</Badge>
											))}
											{(role.permissions?.length || 0) > 3 && (
												<Badge size="sm" variant="light">
													+{role.permissions.length - 3} more
												</Badge>
											)}
										</Group>
									</Table.Td>
									<Table.Td>
										<Badge
											variant={role.isSystem ? "filled" : "light"}
											color={role.isSystem ? "gray" : "blue"}
										>
											{role.isSystem ? "System" : "Custom"}
										</Badge>
									</Table.Td>
									<Table.Td>
										<Badge color={role.active ? "green" : "gray"} variant="light">
											{role.active ? "Active" : "Inactive"}
										</Badge>
									</Table.Td>
									<Table.Td>
										<Menu position="bottom-end" withinPortal>
											<Menu.Target>
												<ActionIcon variant="subtle" color="gray">
													<IconDotsVertical size={16} />
												</ActionIcon>
											</Menu.Target>
											<Menu.Dropdown>
												<Menu.Item
													leftSection={<IconEdit size={14} />}
													onClick={() => handleEdit(role)}
													disabled={role.isSystem}
												>
													Edit
												</Menu.Item>
												<Menu.Divider />
												<Menu.Item
													leftSection={<IconTrash size={14} />}
													color="red"
													onClick={() => handleDeleteClick(role)}
													disabled={role.isSystem}
												>
													Delete
												</Menu.Item>
											</Menu.Dropdown>
										</Menu>
									</Table.Td>
								</Table.Tr>
							))
						)}
					</Table.Tbody>
				</Table>
			</Card>

			<RoleModal opened={opened} onClose={handleClose} role={editingRole} />

			<DeleteRoleModal
				opened={!!deleteTarget}
				onClose={() => setDeleteTarget(null)}
				onConfirm={handleDeleteConfirm}
				roleName={deleteTarget?.name ?? ""}
				isDeleting={deleteRole.isPending}
			/>
		</Stack>
	);
}

function RoleModal({
	opened,
	onClose,
	role,
}: {
	opened: boolean;
	onClose: () => void;
	role: RoleResource | null;
}) {
	const create = useCreateRole();
	const update = useUpdateRole();
	const { data: permissions } = usePermissions();
	const isEditing = !!role;

	const form = useForm({
		initialValues: {
			name: "",
			description: "",
			permissions: [] as string[],
			active: true,
		},
		validate: {
			name: (value) => (value.length < 2 ? "Name must be at least 2 characters" : null),
		},
	});

	// Reset form when modal opens/closes or role changes
	useEffect(() => {
		if (opened) {
			if (role) {
				form.setValues({
					name: role.name,
					description: role.description ?? "",
					permissions: role.permissions ?? [],
					active: role.active,
				});
			} else {
				form.reset();
			}
		}
	}, [opened, role, form.setValues, form.reset]);

	// Group permissions by category
	const groupedPermissions = useMemo(() => {
		const perms = permissions || DEFAULT_PERMISSIONS;
		const groups: Record<string, Permission[]> = {};
		for (const perm of perms) {
			if (!groups[perm.category]) {
				groups[perm.category] = [];
			}
			groups[perm.category].push(perm);
		}
		return groups;
	}, [permissions]);

	const handleSubmit = async (values: typeof form.values) => {
		const roleData: Partial<RoleResource> = {
			resourceType: "Role",
			name: values.name,
			description: values.description || undefined,
			permissions: values.permissions,
			active: values.active,
		};

		try {
			if (isEditing && role?.id) {
				await update.mutateAsync({ ...roleData, id: role.id } as RoleResource);
			} else {
				await create.mutateAsync(roleData);
			}
			onClose();
		} catch {
			// Error handled by mutation hook
		}
	};

	const handleTogglePermission = (permCode: string, checked: boolean) => {
		const currentPerms = form.values.permissions;
		if (checked) {
			form.setFieldValue("permissions", [...currentPerms, permCode]);
		} else {
			form.setFieldValue(
				"permissions",
				currentPerms.filter((p) => p !== permCode)
			);
		}
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit Role" : "Create Role"}
			size="lg"
		>
			<form onSubmit={form.onSubmit(handleSubmit)}>
				<Stack gap="md">
					<TextInput label="Role Name" required {...form.getInputProps("name")} />

					<Textarea
						label="Description"
						placeholder="Optional description for this role"
						{...form.getInputProps("description")}
					/>

					<div>
						<Text size="sm" fw={500} mb="xs">
							Permissions
						</Text>
						<div className={classes.permissionMatrix}>
							{Object.entries(groupedPermissions).map(([category, perms]) => (
								<div key={category}>
									<Text className={classes.categoryHeader}>{category}</Text>
									{perms.map((perm) => (
										<div key={perm.code} className={classes.permissionItem}>
											<Checkbox
												size="xs"
												checked={form.values.permissions.includes(perm.code)}
												onChange={(e) =>
													handleTogglePermission(perm.code, e.currentTarget.checked)
												}
											/>
											<Text className={classes.permissionLabel}>{perm.display}</Text>
										</div>
									))}
								</div>
							))}
						</div>
					</div>

					<Checkbox
						label="Active"
						description="Role can be assigned to users"
						{...form.getInputProps("active", { type: "checkbox" })}
					/>

					<Group justify="flex-end" mt="md">
						<Button variant="light" onClick={onClose}>
							Cancel
						</Button>
						<Button type="submit" loading={create.isPending || update.isPending}>
							{isEditing ? "Update" : "Create"}
						</Button>
					</Group>
				</Stack>
			</form>
		</Modal>
	);
}

function DeleteRoleModal({
	opened,
	onClose,
	onConfirm,
	roleName,
	isDeleting,
}: {
	opened: boolean;
	onClose: () => void;
	onConfirm: () => void;
	roleName: string;
	isDeleting: boolean;
}) {
	return (
		<Modal opened={opened} onClose={onClose} title="Delete Role" size="md">
			<Stack gap="md">
				<Text size="sm">
					You are about to delete the role: <strong>{roleName}</strong>
				</Text>

				<Alert
					icon={<IconAlertTriangle size={20} />}
					color="red"
					variant="light"
				>
					<Text size="sm" fw={500}>
						This action cannot be undone.
					</Text>
					<Text size="sm" c="dimmed">
						Users with this role will lose the associated permissions.
					</Text>
				</Alert>

				<Group justify="flex-end" gap="sm">
					<Button variant="light" onClick={onClose} disabled={isDeleting}>
						Cancel
					</Button>
					<Button color="red" onClick={onConfirm} loading={isDeleting}>
						Delete Role
					</Button>
				</Group>
			</Stack>
		</Modal>
	);
}
