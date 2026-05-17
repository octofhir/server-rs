import { useMemo, useState } from "react";
import {
	Stack,
	Text,
	Group,
	DataPreview,
	Badge,
	Menu,
	Checkbox,
	Textarea,
	Alert,
} from "@/shared/ui";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { Field, Form, type FormApi, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import {
	Plus,
	Magnifier,
	EllipsisVertical,
	Pencil,
	TrashBin,
	Shield,
	TriangleExclamation,
} from "@gravity-ui/icons";
import {
	defaultRolePermissions,
	getRolePermissionPreview,
	getRoleStatusView,
	getRoleTypeView,
	groupRolePermissions,
} from "@/entities/access-role";
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
} from "../lib/useRoles";
import type { RoleResource } from "@/shared/api/types";
import { getBundleResources } from "@/shared/api/guards";
import classes from "./RolesPage.module.css";

export function RolesPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingRole, setEditingRole] = useState<RoleResource | null>(null);
	const [deleteTarget, setDeleteTarget] = useState<RoleResource | null>(null);

	const { data, isLoading } = useRoles({ search: debouncedSearch || undefined });
	const deleteRole = useDeleteRole();

	const roles = getBundleResources<RoleResource>(data);

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
		<WorkspacePageLayout
			title="Roles"
			description="Manage roles and permissions"
			actions={
				<Button leftSection={<Plus size={16} />} onClick={open}>
					Create Role
				</Button>
			}
			toolbar={
				<div className={classes.toolbar}>
					<TextInput
						placeholder="Search roles..."
						leftSection={<Magnifier size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						className={classes.search}
					/>
				</div>
			}
		>

			<Card className={classes.tableContainer}>
				<DataPreview
					columns={[
						{ id: "role", label: "Role" },
						{ id: "permissions", label: "Permissions" },
						{ id: "type", label: "Type", width: 110 },
						{ id: "status", label: "Status", width: 110 },
						{ id: "actions", label: "", width: 48 },
					]}
					rows={
						isLoading
							? []
							: roles.map((role) => {
									const permissionPreview = getRolePermissionPreview(role);
									const typeView = getRoleTypeView(role);
									const statusView = getRoleStatusView(role);

									return {
										role: (
											<div className={classes.roleInfo}>
												<div className={classes.roleIcon}>
													<Shield size={18} />
												</div>
												<div className={classes.roleText}>
													<Text className={classes.roleName}>{role.name}</Text>
													<Text className={classes.roleDescription}>
														{role.description || "No description"}
													</Text>
												</div>
											</div>
										),
										permissions: (
											<div className={classes.badgeList}>
												{permissionPreview.visible.map((permission) => (
													<Badge key={permission} size="sm" variant="dot">
														{permission}
													</Badge>
												))}
												{permissionPreview.remaining > 0 && (
													<Badge size="sm" variant="light">
														+{permissionPreview.remaining} more
													</Badge>
												)}
											</div>
										),
										type: (
											<Badge variant={typeView.variant} color={typeView.color}>
												{typeView.label}
											</Badge>
										),
										status: (
											<Badge color={statusView.color} variant="light">
												{statusView.label}
											</Badge>
										),
										actions: (
											<Menu position="bottom-end" withinPortal>
												<Menu.Target>
													<ActionIcon variant="subtle" color="gray">
														<EllipsisVertical size={16} />
													</ActionIcon>
												</Menu.Target>
												<Menu.Dropdown>
													<Menu.Item
														leftSection={<Pencil size={14} />}
														onClick={() => handleEdit(role)}
														disabled={role.isSystem}
													>
														Edit
													</Menu.Item>
													<Menu.Divider />
													<Menu.Item
														leftSection={<TrashBin size={14} />}
														color="red"
														onClick={() => handleDeleteClick(role)}
														disabled={role.isSystem}
													>
														Delete
													</Menu.Item>
												</Menu.Dropdown>
											</Menu>
										),
									};
								})
					}
					emptyText={isLoading ? "Loading roles..." : "No roles found"}
					getRowKey={(_row, index) => roles[index]?.id ?? roles[index]?.name ?? `${index}`}
				/>
			</Card>

			<RoleModal opened={opened} onClose={handleClose} role={editingRole} />

			<DeleteRoleModal
				opened={!!deleteTarget}
				onClose={() => setDeleteTarget(null)}
				onConfirm={handleDeleteConfirm}
				roleName={deleteTarget?.name ?? ""}
				isDeleting={deleteRole.isPending}
			/>
		</WorkspacePageLayout>
	);
}

interface RoleFormValues {
	name: string;
	description: string;
	permissions: string[];
	active: boolean;
}

const ROLE_FORM_DEFAULTS: RoleFormValues = {
	name: "",
	description: "",
	permissions: [],
	active: true,
};

function validateRoleForm(values: RoleFormValues) {
	const errors: Partial<Record<keyof RoleFormValues, string>> = {};
	if (!values.name || values.name.length < 2) {
		errors.name = "Name must be at least 2 characters";
	}
	return errors;
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

	const initialValues: RoleFormValues = role
		? {
				name: role.name,
				description: role.description ?? "",
				permissions: role.permissions ?? [],
				active: role.active,
			}
		: ROLE_FORM_DEFAULTS;

	const groupedPermissions = useMemo(() => {
		return groupRolePermissions(permissions || defaultRolePermissions);
	}, [permissions]);

	const handleSubmit = async (values: RoleFormValues) => {
		const roleData = {
			resourceType: "Role",
			name: values.name,
			description: values.description || undefined,
			permissions: values.permissions,
			active: values.active,
		} satisfies Partial<RoleResource>;
		try {
			if (isEditing && role?.id) {
				await update.mutateAsync({
					...role,
					...roleData,
					id: role.id,
					resourceType: "Role",
				});
			} else {
				await create.mutateAsync(roleData);
			}
			onClose();
		} catch {
			/* surfaced by mutation hook */
		}
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit Role" : "Create Role"}
			size="lg"
		>
			<Form<RoleFormValues>
				key={role?.id ?? "new"}
				onSubmit={handleSubmit}
				validate={validateRoleForm}
				initialValues={initialValues}
				render={({ handleSubmit: submit, values, form: api, submitting }) => (
					<form onSubmit={submit}>
						<Stack gap="md">
							<Field<string> name="name">
								{({ input, meta }) => (
									<TextInput
										label="Role Name"
										required
										value={input.value}
										onChange={input.onChange}
										onBlur={input.onBlur}
										error={meta.touched && meta.error ? meta.error : undefined}
									/>
								)}
							</Field>

							<Field<string> name="description">
								{({ input }) => (
									<Textarea
										label="Description"
										placeholder="Optional description for this role"
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

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
														checked={values.permissions.includes(perm.code)}
														onChange={(e) =>
															togglePermission(api, values.permissions, perm.code, e.currentTarget.checked)
														}
													/>
													<Text className={classes.permissionLabel}>{perm.display}</Text>
												</div>
											))}
										</div>
									))}
								</div>
							</div>

							<Field<boolean> name="active" type="checkbox">
								{({ input }) => (
									<Checkbox
										label="Active"
										description="Role can be assigned to users"
										checked={input.checked ?? false}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Group justify="flex-end" mt="md">
								<Button variant="light" onClick={onClose} type="button">
									Cancel
								</Button>
								<Button
									type="submit"
									loading={submitting || create.isPending || update.isPending}
								>
									{isEditing ? "Update" : "Create"}
								</Button>
							</Group>
						</Stack>
					</form>
				)}
			/>
		</Modal>
	);
}

function togglePermission(
	api: FormApi<RoleFormValues>,
	current: string[],
	code: string,
	checked: boolean,
) {
	const next = checked ? [...current, code] : current.filter((p) => p !== code);
	api.change("permissions", next);
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
					icon={<TriangleExclamation size={20} />}
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
