import { Button, Card, Field, Form, Modal, TextInput, type FormApi, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import { useMemo, useState } from "react";
import {
	Text,
	DataPreview,
	Badge,
	Checkbox,
	EmptyState,
	Skeleton,
	Textarea,
	Alert,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { DropdownMenu } from "@octofhir/ui-kit";
import { Plus, Search as Magnifier, EllipsisVertical, Pencil, Trash2 as TrashBin, Shield } from "lucide-react";
import {
	defaultRolePermissions,
	getRolePermissionPreview,
	getRoleStatusView,
	getRoleTypeView,
	groupRolePermissions,
} from "@/entities/access-role";
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

	const { data, isLoading, isError, error, refetch } = useRoles({ search: debouncedSearch || undefined });
	const deleteRole = useDeleteRole();

	const roles = getBundleResources<RoleResource>(data);
	const isFiltered = debouncedSearch.length > 0;

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
				<Button view="action" onClick={open}>
					<Button.Icon>
						<Plus width={16} />
					</Button.Icon>
					Create Role
				</Button>
			}
			toolbar={
				<div className={classes.toolbar}>
					<TextInput
						aria-label="Search roles by name"
						placeholder="Search roles..."
						leftSection={<Magnifier width={16} />}
						value={search}
						onChange={(value) => setSearch(value)}
						className={classes.search}
					/>
				</div>
			}
		>

			<Card className={classes.tableContainer}>
				{isLoading ? (
					<div className={classes.skeletonList}>
						{["a", "b", "c", "d", "e"].map((k) => (
							<Skeleton key={k} className={classes.skeletonRow} />
						))}
					</div>
				) : isError ? (
					<EmptyState
						title="Failed to load roles"
						description={error instanceof Error ? error.message : "Something went wrong while loading roles."}
						actions={[
							<Button key="retry" view="action" onClick={() => refetch()}>
								Retry
							</Button>,
						]}
					/>
				) : roles.length === 0 ? (
					<EmptyState
						title={isFiltered ? "No matching roles" : "No roles yet"}
						description={
							isFiltered
								? "No roles match your search. Try a different term."
								: "Create roles to group permissions and assign them to users."
						}
						actions={
							isFiltered
								? [
										<Button key="clear" view="outlined" onClick={() => setSearch("")}>
											Clear filters
										</Button>,
									]
								: [
										<Button key="create" view="action" onClick={open}>
											Create Role
										</Button>,
									]
						}
					/>
				) : (
					<DataPreview
						columns={[
							{ id: "role", label: "Role" },
							{ id: "permissions", label: "Permissions" },
							{ id: "type", label: "Type", width: 110 },
							{ id: "status", label: "Status", width: 110 },
							{ id: "actions", label: "", width: 48 },
						]}
						rows={roles.map((role) => {
							const permissionPreview = getRolePermissionPreview(role);
							const typeView = getRoleTypeView(role);
							const statusView = getRoleStatusView(role);

							return {
								role: (
									<div className={classes.roleInfo}>
										<div className={classes.roleIcon}>
											<Shield width={18} height={18} aria-hidden="true" />
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
											<Badge key={permission} size="sm">
												{permission}
											</Badge>
										))}
										{permissionPreview.remaining > 0 && (
											<Badge size="sm">+{permissionPreview.remaining} more</Badge>
										)}
									</div>
								),
								type: <Badge color={typeView.color}>{typeView.label}</Badge>,
								status: <Badge color={statusView.color}>{statusView.label}</Badge>,
								actions: (
									<DropdownMenu
										size="s"
										icon={<EllipsisVertical width={16} />}
										defaultSwitcherProps={{
											view: "flat-secondary",
											size: "s",
											"aria-label": "Role actions",
											"aria-haspopup": "menu",
										}}
										popupProps={{ placement: "bottom-end" }}
										items={[
											{
												text: "Edit",
												iconStart: <Pencil width={14} />,
												disabled: role.isSystem,
												action: () => handleEdit(role),
											},
											[
												{
													text: "Delete",
													iconStart: <TrashBin width={14} />,
													theme: "danger",
													disabled: role.isSystem,
													action: () => handleDeleteClick(role),
												},
											],
										]}
									/>
								),
							};
						})}
						getRowKey={(_row, index) => roles[index]?.id ?? roles[index]?.name ?? `${index}`}
					/>
				)}
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
						<div className={classes.roleForm}>
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
								<Text variant="body-2" className={classes.sectionLabel}>
									<strong>Permissions</strong>
								</Text>
								<div className={classes.permissionMatrix}>
									{Object.entries(groupedPermissions).map(([category, perms]) => (
										<div key={category}>
											<Text className={classes.categoryHeader}>{category}</Text>
											{perms.map((perm) => (
												<div key={perm.code} className={classes.permissionItem}>
													<Checkbox
														size="s"
														content={perm.display}
														checked={values.permissions.includes(perm.code)}
														onChange={(e) =>
															togglePermission(api, values.permissions, perm.code, e.currentTarget.checked)
														}
													/>
												</div>
											))}
										</div>
									))}
								</div>
							</div>

							<Field<boolean> name="active" type="checkbox">
								{({ input }) => (
									<div className={classes.switchField}>
										<Checkbox
											content="Active"
											checked={input.checked ?? false}
											onChange={input.onChange}
										/>
										<Text variant="caption-2" color="secondary">
											Role can be assigned to users
										</Text>
									</div>
								)}
							</Field>

							<div className={classes.formActions}>
								<Button view="flat-secondary" onClick={onClose} type="button">
									Cancel
								</Button>
								<Button
									view="action"
									type="submit"
									loading={submitting || create.isPending || update.isPending}
								>
									{isEditing ? "Update" : "Create"}
								</Button>
							</div>
						</div>
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
			<div className={classes.deleteModalContent}>
				<Text variant="body-2">
					You are about to delete the role: <strong>{roleName}</strong>
				</Text>

				<Alert
					theme="danger"
					title="This action cannot be undone."
					message="Users with this role will lose the associated permissions."
				/>

				<div className={classes.formActions}>
					<Button view="flat-secondary" onClick={onClose} disabled={isDeleting}>
						Cancel
					</Button>
					<Button view="flat-danger" onClick={onConfirm} loading={isDeleting}>
						Delete Role
					</Button>
				</div>
			</div>
		</Modal>
	);
}
