import { Button, Field, Form, FormSpy, Modal, TextInput, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import { useState } from "react";
import {
	Text,
	Table,
	Badge,
	Checkbox,
	EmptyState,
	Select,
	PasswordInput,
	Skeleton,
	Switch,
	Card,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { DropdownMenu } from "@gravity-ui/uikit";
import { useNavigate } from "react-router-dom";
import {
	Plus,
	Magnifier,
	EllipsisVertical,
	Pencil,
	TrashBin,
	Key,
	Eye,
	PersonPencil,
	PersonXmark,
} from "@gravity-ui/icons";
import {
	formatUserLastLogin,
	getPasswordStrength,
	getUserInitials,
	getUserRoleView,
	getUserStatusView,
} from "@/entities/user-account";
import {
	useUsers,
	useCreateUser,
	useUpdateUser,
	useDeleteUser,
	useResetPassword,
	useBulkUpdateUsers,
	type UserFilterParams,
} from "../lib/useUsers";
import { useRoles } from "../lib/useRoles";
import type { RoleResource, UserResource } from "@/shared/api/types";
import { getBundleResources } from "@/shared/api/guards";
import { DeleteUserModal } from "./DeleteUserModal";
import classes from "./UsersPage.module.css";

export function UsersPage() {
	const navigate = useNavigate();
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [roleFilter, setRoleFilter] = useState<string | null>(null);
	const [statusFilter, setStatusFilter] = useState<string | null>(null);
	const [opened, { open, close }] = useDisclosure(false);
	const [resetPasswordOpened, { open: openResetPassword, close: closeResetPassword }] = useDisclosure(false);
	const [editingUser, setEditingUser] = useState<UserResource | null>(null);
	const [deleteTarget, setDeleteTarget] = useState<UserResource | null>(null);
	const [resetPasswordTarget, setResetPasswordTarget] = useState<UserResource | null>(null);
	const [selectedUsers, setSelectedUsers] = useState<Set<string>>(new Set());

	const filters: UserFilterParams = {
		search: debouncedSearch || undefined,
		role: roleFilter || undefined,
		active: statusFilter === "active" ? true : statusFilter === "inactive" ? false : undefined,
	};

	const { data, isLoading, isError, error, refetch } = useUsers(filters);
	const { data: rolesData } = useRoles();
	const deleteUser = useDeleteUser();
	const bulkUpdate = useBulkUpdateUsers();

	const users = getBundleResources<UserResource>(data);
	const isFiltered = Boolean(debouncedSearch || roleFilter || statusFilter);
	const clearFilters = () => {
		setSearch("");
		setRoleFilter(null);
		setStatusFilter(null);
	};
	const roleNames = getBundleResources<RoleResource>(rolesData).map((role) => role.name);
	const availableRoles = roleNames.length ? roleNames : ["admin", "practitioner", "patient"];

	const handleEdit = (user: UserResource) => {
		setEditingUser(user);
		open();
	};

	const handleDeleteClick = (user: UserResource) => {
		setDeleteTarget(user);
	};

	const handleDeleteConfirm = () => {
		if (deleteTarget?.id) {
			deleteUser.mutate(deleteTarget.id, {
				onSuccess: () => setDeleteTarget(null),
			});
		}
	};

	const handleResetPasswordClick = (user: UserResource) => {
		setResetPasswordTarget(user);
		openResetPassword();
	};

	const handleViewDetails = (user: UserResource) => {
		navigate(`/auth/users/${user.id}`);
	};

	const handleClose = () => {
		setEditingUser(null);
		close();
	};

	const handleSelectUser = (userId: string, checked: boolean) => {
		setSelectedUsers((prev) => {
			const next = new Set(prev);
			if (checked) {
				next.add(userId);
			} else {
				next.delete(userId);
			}
			return next;
		});
	};

	const handleSelectAll = (checked: boolean) => {
		if (checked) {
			setSelectedUsers(new Set(users.map((u) => u.id).filter((id): id is string => !!id)));
		} else {
			setSelectedUsers(new Set());
		}
	};

	const handleBulkActivate = () => {
		bulkUpdate.mutate(
			{ userIds: Array.from(selectedUsers), updates: { active: true } },
			{ onSuccess: () => setSelectedUsers(new Set()) }
		);
	};

	const handleBulkDeactivate = () => {
		bulkUpdate.mutate(
			{ userIds: Array.from(selectedUsers), updates: { active: false } },
			{ onSuccess: () => setSelectedUsers(new Set()) }
		);
	};

	const allSelected = users.length > 0 && users.every((u) => u.id && selectedUsers.has(u.id));
	const someSelected = selectedUsers.size > 0;

	return (
		<WorkspacePageLayout
			title="Users"
			description="Manage user accounts, roles, and credentials"
			actions={
				<Button view="action" onClick={open}>
					<Button.Icon>
						<Plus width={16} />
					</Button.Icon>
					Create User
				</Button>
			}
			toolbar={
				<div className={classes.filterBar}>
					<TextInput
						placeholder="Search by username or email..."
						startContent={<Magnifier width={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						className={classes.searchInput}
					/>
					<Select
						placeholder="Filter by role"
						options={availableRoles.map((r) => ({ value: r, content: r }))}
						value={roleFilter ? [roleFilter] : []}
						onUpdate={(value) => setRoleFilter(value[0] ?? null)}
						hasClear
						filterable
						width={180}
					/>
					<Select
						placeholder="Status"
						options={[
							{ value: "active", content: "Active" },
							{ value: "inactive", content: "Inactive" },
						]}
						value={statusFilter ? [statusFilter] : []}
						onUpdate={(value) => setStatusFilter(value[0] ?? null)}
						hasClear
						width={140}
					/>
				</div>
			}
		>

			<Card type="container" view="outlined" className={classes.tableContainer}>
				{someSelected && (
					<div className={classes.bulkActions}>
						<Text variant="body-1" className={classes.selectedCount}>
							{selectedUsers.size} user{selectedUsers.size !== 1 ? "s" : ""} selected
						</Text>
						<Button
							size="xs"
							view="outlined-success"
							onClick={handleBulkActivate}
							loading={bulkUpdate.isPending}
						>
							<Button.Icon>
								<PersonPencil width={14} />
							</Button.Icon>
							Activate
						</Button>
						<Button
							size="xs"
							view="outlined-warning"
							onClick={handleBulkDeactivate}
							loading={bulkUpdate.isPending}
						>
							<Button.Icon>
								<PersonXmark width={14} />
							</Button.Icon>
							Deactivate
						</Button>
						<Button
							size="xs"
							view="flat-secondary"
							onClick={() => setSelectedUsers(new Set())}
						>
							Clear selection
						</Button>
					</div>
				)}

				<Table.ScrollContainer minWidth={860}>
					<Table verticalSpacing="sm" highlightOnHover>
						<Table.Thead>
							<Table.Tr>
								<Table.Th className={classes.selectionCell}>
									<Checkbox
										checked={allSelected}
										indeterminate={someSelected && !allSelected}
										onChange={(e) => handleSelectAll(e.currentTarget.checked)}
									/>
								</Table.Th>
								<Table.Th>User</Table.Th>
								<Table.Th>Roles</Table.Th>
								<Table.Th>Status</Table.Th>
								<Table.Th>Last login</Table.Th>
								<Table.Th className={classes.actionsCell} />
							</Table.Tr>
						</Table.Thead>
						<Table.Tbody>
							{isLoading ? (
								["a", "b", "c", "d", "e"].map((k) => (
									<Table.Tr key={k}>
										<Table.Td colSpan={6}>
											<Skeleton className={classes.skeletonRow} />
										</Table.Td>
									</Table.Tr>
								))
							) : isError ? (
								<Table.Tr>
									<Table.Td colSpan={6} className={classes.emptyCell}>
										<EmptyState
											title="Failed to load users"
											description={
												error instanceof Error ? error.message : "Something went wrong while loading users."
											}
											actions={[
												<Button key="retry" view="action" onClick={() => refetch()}>
													Retry
												</Button>,
											]}
										/>
									</Table.Td>
								</Table.Tr>
							) : users.length === 0 ? (
								<Table.Tr>
									<Table.Td colSpan={6} className={classes.emptyCell}>
										<EmptyState
											title={isFiltered ? "No matching users" : "No users yet"}
											description={
												isFiltered
													? "No users match your current filters."
													: "Create a user account to grant access to the server."
											}
											actions={
												isFiltered
													? [
															<Button key="clear" view="outlined" onClick={clearFilters}>
																Clear filters
															</Button>,
														]
													: [
															<Button key="create" view="action" onClick={open}>
																Create User
															</Button>,
														]
											}
										/>
									</Table.Td>
								</Table.Tr>
							) : (
								users.map((user) => {
									const statusView = getUserStatusView(user);

									return (
									<Table.Tr key={user.id}>
										<Table.Td className={classes.selectionCell}>
											<Checkbox
												checked={user.id ? selectedUsers.has(user.id) : false}
												onChange={(e) => user.id && handleSelectUser(user.id, e.currentTarget.checked)}
											/>
										</Table.Td>
										<Table.Td>
											<div className={classes.userCell}>
												<div className={classes.avatar}>{getUserInitials(user)}</div>
												<div className={classes.userInfo}>
													<Text variant="body-2" className={classes.userName}>
														{user.name || user.username}
													</Text>
													<Text variant="body-1" color="secondary" className={classes.userEmail}>
														{user.email || user.username}
													</Text>
												</div>
											</div>
										</Table.Td>
										<Table.Td>
											<div className={classes.roleList}>
												{user.roles?.map((role) => {
													const roleView = getUserRoleView(role);
													return (
													<Badge
														key={role}
														size="s"
														theme={roleView.theme}
													>
														{roleView.role}
													</Badge>
													);
												})}
												{(!user.roles || user.roles.length === 0) && (
													<Text variant="body-1" color="secondary">
														No roles
													</Text>
												)}
											</div>
										</Table.Td>
										<Table.Td>
											<Badge
												size="s"
												theme={statusView.theme}
											>
												{statusView.label}
											</Badge>
										</Table.Td>
										<Table.Td>
											<Text variant="body-1" color="secondary" className={classes.lastLogin}>
												{formatUserLastLogin(user.lastLogin)}
											</Text>
										</Table.Td>
										<Table.Td className={classes.actionsCell}>
											<DropdownMenu
												size="s"
												icon={<EllipsisVertical width={16} />}
												defaultSwitcherProps={{ view: "flat-secondary", size: "s", "aria-label": "User actions" }}
												popupProps={{ placement: "bottom-end" }}
												items={[
													{
														text: "View details",
														iconStart: <Eye width={14} />,
														action: () => handleViewDetails(user),
													},
													{
														text: "Edit",
														iconStart: <Pencil width={14} />,
														action: () => handleEdit(user),
													},
													{
														text: "Reset password",
														iconStart: <Key width={14} />,
														action: () => handleResetPasswordClick(user),
													},
													[
														{
															text: "Delete",
															iconStart: <TrashBin width={14} />,
															theme: "danger",
															action: () => handleDeleteClick(user),
														},
													],
												]}
											/>
										</Table.Td>
									</Table.Tr>
									);
								})
							)}
						</Table.Tbody>
					</Table>
				</Table.ScrollContainer>
			</Card>

			<UserModal
				opened={opened}
				onClose={handleClose}
				user={editingUser}
				availableRoles={availableRoles}
			/>

			<DeleteUserModal
				opened={!!deleteTarget}
				onClose={() => setDeleteTarget(null)}
				onConfirm={handleDeleteConfirm}
				userName={deleteTarget?.name || deleteTarget?.username || ""}
				userEmail={deleteTarget?.email || ""}
				isDeleting={deleteUser.isPending}
			/>

			<ResetPasswordModal
				opened={resetPasswordOpened}
				onClose={() => {
					closeResetPassword();
					setResetPasswordTarget(null);
				}}
				user={resetPasswordTarget}
			/>
		</WorkspacePageLayout>
	);
}

function UserModal({
	opened,
	onClose,
	user,
	availableRoles,
}: {
	opened: boolean;
	onClose: () => void;
	user: UserResource | null;
	availableRoles: string[];
}) {
	const create = useCreateUser();
	const update = useUpdateUser();
	const isEditing = !!user;

	const initialValues: UserFormValues = user
		? {
				username: user.username,
				name: user.name ?? "",
				email: user.email ?? "",
				password: "",
				active: user.active,
				roles: user.roles ?? [],
				mfaEnabled: user.mfaEnabled ?? false,
			}
		: USER_DEFAULTS;

	const validate = (values: UserFormValues) => {
		const errors: Partial<Record<keyof UserFormValues, string>> = {};
		if (!values.username || values.username.length < 3)
			errors.username = "Username must be at least 3 characters";
		if (values.email && !/^\S+@\S+$/.test(values.email)) errors.email = "Invalid email";
		if (!isEditing && (!values.password || values.password.length < 8))
			errors.password = "Password must be at least 8 characters";
		else if (values.password && getPasswordStrength(values.password).score < 2)
			errors.password = "Password is too weak";
		return errors;
	};

	const handleSubmit = async (values: UserFormValues) => {
		const userData: Partial<UserResource> = {
			resourceType: "User",
			username: values.username,
			name: values.name || undefined,
			email: values.email || undefined,
			active: values.active,
			roles: values.roles,
			mfaEnabled: values.mfaEnabled,
		};
		if (values.password) userData.password = values.password;
		try {
			if (isEditing && user?.id) {
				await update.mutateAsync({
					...user,
					...userData,
					id: user.id,
					resourceType: "User",
				});
			} else {
				await create.mutateAsync(userData);
			}
			onClose();
		} catch {
			/* surfaced by mutation */
		}
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit User" : "Create User"}
			size="lg"
		>
			<Form<UserFormValues>
				key={user?.id ?? "new"}
				onSubmit={handleSubmit}
				validate={validate}
				initialValues={initialValues}
				render={({ handleSubmit: submit, submitting }) => (
					<form onSubmit={submit}>
						<div className={classes.modalForm}>
							<div className={classes.formGrid}>
								<Field<string> name="username">
									{({ input, meta }) => (
										<TextInput
											label="Username"
											required
											value={input.value}
											onChange={input.onChange}
											onBlur={input.onBlur}
											disabled={isEditing}
											error={meta.touched && meta.error ? meta.error : undefined}
										/>
									)}
								</Field>
								<Field<string> name="name">
									{({ input }) => (
										<TextInput label="Full Name" value={input.value} onChange={input.onChange} />
									)}
								</Field>
							</div>

							<Field<string> name="email">
								{({ input, meta }) => (
									<TextInput
										label="Email"
										type="email"
										value={input.value}
										onChange={input.onChange}
										onBlur={input.onBlur}
										error={meta.touched && meta.error ? meta.error : undefined}
									/>
								)}
							</Field>

							<div>
								<Field<string> name="password">
									{({ input, meta }) => (
										<PasswordInput
											label={isEditing ? "New Password" : "Password"}
											placeholder={
												isEditing ? "Leave blank to keep current" : "Enter password"
											}
											required={!isEditing}
											value={input.value}
											onChange={input.onChange}
											onBlur={input.onBlur}
											error={meta.touched && meta.error ? meta.error : undefined}
										/>
									)}
								</Field>
								<FormSpy<UserFormValues> subscription={{ values: true }}>
									{({ values: v }) => {
										if (!v.password) return null;
										const strength = getPasswordStrength(v.password);
										return (
											<>
												<div className={classes.passwordStrength}>
													{[0, 1, 2, 3].map((i) => (
														<div
															key={i}
															className={classes.strengthBar}
															data-active={i < strength.score}
															data-strength={strength.label}
														/>
													))}
												</div>
												<Text
													className={classes.strengthLabel}
													color={
														strength.label === "weak"
															? "danger"
															: strength.label === "fair"
																? "warning"
																: strength.label === "good"
																	? "warning"
																	: "positive"
													}
												>
													Password strength: {strength.label}
												</Text>
											</>
										);
									}}
								</FormSpy>
							</div>

							<Field<string[]> name="roles">
								{({ input }) => (
									<Select
										label="Roles"
										options={availableRoles.map((r) => ({ value: r, content: r }))}
										filterable
										multiple
										value={input.value}
										onUpdate={input.onChange}
									/>
								)}
							</Field>

							<div className={classes.switchGrid}>
								<Field<boolean> name="active" type="checkbox">
									{({ input }) => (
										<div className={classes.switchCard}>
											<Switch
												content="Active"
												checked={input.checked ?? false}
												onUpdate={input.onChange}
											/>
											<Text color="secondary" variant="body-1">User can log in</Text>
										</div>
									)}
								</Field>
								<Field<boolean> name="mfaEnabled" type="checkbox">
									{({ input }) => (
										<div className={classes.switchCard}>
											<Switch
												content="MFA enabled"
												checked={input.checked ?? false}
												onUpdate={input.onChange}
											/>
											<Text color="secondary" variant="body-1">Require two-factor authentication</Text>
										</div>
									)}
								</Field>
							</div>

							<div className={classes.modalActions}>
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

interface UserFormValues {
	username: string;
	name: string;
	email: string;
	password: string;
	active: boolean;
	roles: string[];
	mfaEnabled: boolean;
}

const USER_DEFAULTS: UserFormValues = {
	username: "",
	name: "",
	email: "",
	password: "",
	active: true,
	roles: [],
	mfaEnabled: false,
};

function ResetPasswordModal({
	opened,
	onClose,
	user,
}: {
	opened: boolean;
	onClose: () => void;
	user: UserResource | null;
}) {
	const resetPassword = useResetPassword();

	const validate = (values: ResetPasswordValues) => {
		const errors: Partial<Record<keyof ResetPasswordValues, string>> = {};
		if (values.newPassword.length < 8) errors.newPassword = "Password must be at least 8 characters";
		else if (getPasswordStrength(values.newPassword).score < 2) errors.newPassword = "Password is too weak";
		if (values.confirmPassword !== values.newPassword)
			errors.confirmPassword = "Passwords do not match";
		return errors;
	};

	const handleSubmit = async (values: ResetPasswordValues, api: { reset: () => void }) => {
		if (!user?.id) return;
		try {
			await resetPassword.mutateAsync({ userId: user.id, newPassword: values.newPassword });
			api.reset();
			onClose();
		} catch {
			/* surfaced by mutation */
		}
	};

	return (
		<Modal opened={opened} onClose={onClose} title="Reset Password">
			<Form<ResetPasswordValues>
				onSubmit={(values, api) => handleSubmit(values, api)}
				validate={validate}
				initialValues={{ newPassword: "", confirmPassword: "" }}
				render={({ handleSubmit: submit, submitting }) => (
					<form onSubmit={submit}>
						<div className={classes.modalForm}>
							<Text variant="body-1">
								Reset password for user: <strong>{user?.name || user?.username}</strong>
							</Text>

							<div>
								<Field<string> name="newPassword">
									{({ input, meta }) => (
										<PasswordInput
											label="New Password"
											required
											value={input.value}
											onChange={input.onChange}
											onBlur={input.onBlur}
											error={meta.touched && meta.error ? meta.error : undefined}
										/>
									)}
								</Field>
								<FormSpy<ResetPasswordValues> subscription={{ values: true }}>
									{({ values: v }) => {
										if (!v.newPassword) return null;
										const strength = getPasswordStrength(v.newPassword);
										return (
											<>
												<div className={classes.passwordStrength}>
													{[0, 1, 2, 3].map((i) => (
														<div
															key={i}
															className={classes.strengthBar}
															data-active={i < strength.score}
															data-strength={strength.label}
														/>
													))}
												</div>
												<Text
													className={classes.strengthLabel}
													color={
														strength.label === "weak"
															? "danger"
															: strength.label === "fair"
																? "warning"
																: strength.label === "good"
																	? "warning"
																	: "positive"
													}
												>
													Password strength: {strength.label}
												</Text>
											</>
										);
									}}
								</FormSpy>
							</div>

							<Field<string> name="confirmPassword">
								{({ input, meta }) => (
									<PasswordInput
										label="Confirm Password"
										required
										value={input.value}
										onChange={input.onChange}
										onBlur={input.onBlur}
										error={meta.touched && meta.error ? meta.error : undefined}
									/>
								)}
							</Field>

							<div className={classes.modalActions}>
								<Button view="flat-secondary" onClick={onClose} type="button">
									Cancel
								</Button>
								<Button view="action" type="submit" loading={submitting || resetPassword.isPending}>
									Reset Password
								</Button>
							</div>
						</div>
					</form>
				)}
			/>
		</Modal>
	);
}
