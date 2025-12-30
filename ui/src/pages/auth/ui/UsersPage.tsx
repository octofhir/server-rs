import { useState, useEffect } from "react";
import {
	Stack,
	Title,
	Text,
	Group,
	Table,
	Badge,
	Menu,
	Checkbox,
	Select,
	PasswordInput,
	MultiSelect,
	Switch,
} from "@mantine/core";
import { useDisclosure, useDebouncedValue } from "@mantine/hooks";
import { useForm } from "@mantine/form";
import { useNavigate } from "react-router-dom";
import {
	IconPlus,
	IconSearch,
	IconDotsVertical,
	IconEdit,
	IconTrash,
	IconKey,
	IconEye,
	IconUserCheck,
	IconUserX,
	IconFilter,
} from "@tabler/icons-react";
import { Card } from "@/shared/ui/Card/Card";
import { Modal } from "@/shared/ui/Modal/Modal";
import { Button } from "@/shared/ui/Button/Button";
import { TextInput } from "@/shared/ui/TextInput/TextInput";
import { ActionIcon } from "@/shared/ui/ActionIcon/ActionIcon";
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
import type { UserResource } from "@/shared/api/types";
import { DeleteUserModal } from "./DeleteUserModal";
import classes from "./UsersPage.module.css";

// Password strength calculation
function getPasswordStrength(password: string): { score: number; label: string } {
	let score = 0;
	if (password.length >= 8) score++;
	if (password.length >= 12) score++;
	if (/[a-z]/.test(password) && /[A-Z]/.test(password)) score++;
	if (/\d/.test(password)) score++;
	if (/[^a-zA-Z0-9]/.test(password)) score++;

	const labels = ["weak", "weak", "fair", "good", "strong", "strong"];
	return { score: Math.min(score, 4), label: labels[score] || "weak" };
}

// Get user initials for avatar
function getUserInitials(user: UserResource): string {
	if (user.name) {
		return user.name
			.split(" ")
			.map((n) => n[0])
			.join("")
			.toUpperCase()
			.slice(0, 2);
	}
	return user.username.slice(0, 2).toUpperCase();
}

// Format last login date
function formatLastLogin(date: string | undefined): string {
	if (!date) return "Never";
	const d = new Date(date);
	const now = new Date();
	const diff = now.getTime() - d.getTime();
	const days = Math.floor(diff / (1000 * 60 * 60 * 24));

	if (days === 0) return "Today";
	if (days === 1) return "Yesterday";
	if (days < 7) return `${days} days ago`;
	if (days < 30) return `${Math.floor(days / 7)} weeks ago`;
	return d.toLocaleDateString();
}

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

	const { data, isLoading } = useUsers(filters);
	const { data: rolesData } = useRoles();
	const deleteUser = useDeleteUser();
	const bulkUpdate = useBulkUpdateUsers();

	const users = data?.entry?.map((e) => e.resource) || [];
	const availableRoles = rolesData?.entry?.map((e) => e.resource.name) || ["admin", "practitioner", "patient"];

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
		<Stack gap="md" className={classes.pageRoot}>
			<Group justify="space-between">
				<div>
					<Title order={2}>Users</Title>
					<Text c="dimmed" size="sm">
						Manage user accounts, roles, and credentials
					</Text>
				</div>
				<Button leftSection={<IconPlus size={16} />} onClick={open}>
					Create User
				</Button>
			</Group>

			<Card className={classes.tableContainer}>
				<Group mb="md" className={classes.filterBar}>
					<TextInput
						placeholder="Search by username or email..."
						leftSection={<IconSearch size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						className={classes.searchInput}
					/>
					<Select
						placeholder="Filter by role"
						leftSection={<IconFilter size={16} />}
						data={availableRoles.map((r) => ({ value: r, label: r }))}
						value={roleFilter}
						onChange={setRoleFilter}
						clearable
						w={180}
					/>
					<Select
						placeholder="Status"
						data={[
							{ value: "active", label: "Active" },
							{ value: "inactive", label: "Inactive" },
						]}
						value={statusFilter}
						onChange={setStatusFilter}
						clearable
						w={140}
					/>
				</Group>

				{someSelected && (
					<div className={classes.bulkActions}>
						<Text className={classes.selectedCount}>
							{selectedUsers.size} user{selectedUsers.size !== 1 ? "s" : ""} selected
						</Text>
						<Button
							size="xs"
							variant="light"
							color="green"
							leftSection={<IconUserCheck size={14} />}
							onClick={handleBulkActivate}
							loading={bulkUpdate.isPending}
						>
							Activate
						</Button>
						<Button
							size="xs"
							variant="light"
							color="orange"
							leftSection={<IconUserX size={14} />}
							onClick={handleBulkDeactivate}
							loading={bulkUpdate.isPending}
						>
							Deactivate
						</Button>
						<Button
							size="xs"
							variant="subtle"
							color="gray"
							onClick={() => setSelectedUsers(new Set())}
						>
							Clear selection
						</Button>
					</div>
				)}

				<Table>
					<Table.Thead>
						<Table.Tr>
							<Table.Th style={{ width: 40 }}>
								<Checkbox
									checked={allSelected}
									indeterminate={someSelected && !allSelected}
									onChange={(e) => handleSelectAll(e.currentTarget.checked)}
								/>
							</Table.Th>
							<Table.Th>User</Table.Th>
							<Table.Th>Roles</Table.Th>
							<Table.Th>Status</Table.Th>
							<Table.Th>Last Login</Table.Th>
							<Table.Th style={{ width: 50 }} />
						</Table.Tr>
					</Table.Thead>
					<Table.Tbody>
						{isLoading ? (
							<Table.Tr>
								<Table.Td colSpan={6}>Loading...</Table.Td>
							</Table.Tr>
						) : users.length === 0 ? (
							<Table.Tr>
								<Table.Td colSpan={6} style={{ textAlign: "center" }}>
									No users found
								</Table.Td>
							</Table.Tr>
						) : (
							users.map((user) => (
								<Table.Tr key={user.id}>
									<Table.Td>
										<Checkbox
											checked={user.id ? selectedUsers.has(user.id) : false}
											onChange={(e) => user.id && handleSelectUser(user.id, e.currentTarget.checked)}
										/>
									</Table.Td>
									<Table.Td>
										<div className={classes.userCell}>
											<div className={classes.avatar}>{getUserInitials(user)}</div>
											<div className={classes.userInfo}>
												<Text className={classes.userName}>
													{user.name || user.username}
												</Text>
												<Text className={classes.userEmail}>
													{user.email || user.username}
												</Text>
											</div>
										</div>
									</Table.Td>
									<Table.Td>
										<Group gap={4}>
											{user.roles?.map((role) => (
												<Badge
													key={role}
													size="sm"
													variant={role === "admin" ? "filled" : "dot"}
													color={role === "admin" ? "red" : "blue"}
												>
													{role}
												</Badge>
											))}
											{(!user.roles || user.roles.length === 0) && (
												<Text size="xs" c="dimmed">
													No roles
												</Text>
											)}
										</Group>
									</Table.Td>
									<Table.Td>
										<Badge
											color={user.active ? "green" : user.status === "locked" ? "red" : "gray"}
											variant="light"
										>
											{user.status === "locked" ? "Locked" : user.active ? "Active" : "Inactive"}
										</Badge>
									</Table.Td>
									<Table.Td>
										<Text className={classes.lastLogin}>{formatLastLogin(user.lastLogin)}</Text>
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
													leftSection={<IconEye size={14} />}
													onClick={() => handleViewDetails(user)}
												>
													View Details
												</Menu.Item>
												<Menu.Item
													leftSection={<IconEdit size={14} />}
													onClick={() => handleEdit(user)}
												>
													Edit
												</Menu.Item>
												<Menu.Item
													leftSection={<IconKey size={14} />}
													onClick={() => handleResetPasswordClick(user)}
												>
													Reset Password
												</Menu.Item>
												<Menu.Divider />
												<Menu.Item
													leftSection={<IconTrash size={14} />}
													color="red"
													onClick={() => handleDeleteClick(user)}
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
		</Stack>
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
	const [password, setPassword] = useState("");

	const form = useForm({
		initialValues: {
			username: "",
			name: "",
			email: "",
			password: "",
			active: true,
			roles: [] as string[],
			mfaEnabled: false,
		},
		validate: {
			username: (value) => (value.length < 3 ? "Username must be at least 3 characters" : null),
			email: (value) => (value && !/^\S+@\S+$/.test(value) ? "Invalid email" : null),
			password: (value) => {
				if (!isEditing && value.length < 8) return "Password must be at least 8 characters";
				if (value && getPasswordStrength(value).score < 2) return "Password is too weak";
				return null;
			},
		},
	});

	const passwordStrength = getPasswordStrength(password);

	// Reset form when modal opens/closes or user changes
	useEffect(() => {
		if (opened) {
			if (user) {
				form.setValues({
					username: user.username,
					name: user.name ?? "",
					email: user.email ?? "",
					password: "",
					active: user.active,
					roles: user.roles ?? [],
					mfaEnabled: user.mfaEnabled ?? false,
				});
			} else {
				form.reset();
			}
			setPassword("");
		}
	}, [opened, user, form.setValues, form.reset]);

	const handleSubmit = async (values: typeof form.values) => {
		const userData: Partial<UserResource> = {
			resourceType: "User",
			username: values.username,
			name: values.name || undefined,
			email: values.email || undefined,
			active: values.active,
			roles: values.roles,
			mfaEnabled: values.mfaEnabled,
		};

		if (values.password) {
			userData.password = values.password;
		}

		try {
			if (isEditing && user?.id) {
				await update.mutateAsync({ ...userData, id: user.id } as UserResource);
			} else {
				await create.mutateAsync(userData);
			}
			onClose();
		} catch {
			// Error handled by mutation hook
		}
	};

	return (
		<Modal opened={opened} onClose={onClose} title={isEditing ? "Edit User" : "Create User"} size="lg">
			<form onSubmit={form.onSubmit(handleSubmit)}>
				<Stack gap="md">
					<Group grow>
						<TextInput
							label="Username"
							required
							{...form.getInputProps("username")}
							disabled={isEditing}
						/>
						<TextInput label="Full Name" {...form.getInputProps("name")} />
					</Group>

					<TextInput label="Email" type="email" {...form.getInputProps("email")} />

					<div>
						<PasswordInput
							label={isEditing ? "New Password" : "Password"}
							placeholder={isEditing ? "Leave blank to keep current" : "Enter password"}
							required={!isEditing}
							{...form.getInputProps("password")}
							onChange={(e) => {
								form.setFieldValue("password", e.currentTarget.value);
								setPassword(e.currentTarget.value);
							}}
						/>
						{password && (
							<>
								<div className={classes.passwordStrength}>
									{[0, 1, 2, 3].map((i) => (
										<div
											key={i}
											className={classes.strengthBar}
											data-active={i < passwordStrength.score}
											data-strength={passwordStrength.label}
										/>
									))}
								</div>
								<Text
									className={classes.strengthLabel}
									c={
										passwordStrength.label === "weak"
											? "red"
											: passwordStrength.label === "fair"
												? "orange"
												: passwordStrength.label === "good"
													? "yellow"
													: "green"
									}
								>
									Password strength: {passwordStrength.label}
								</Text>
							</>
						)}
					</div>

					<MultiSelect
						label="Roles"
						data={availableRoles.map((r) => ({ value: r, label: r }))}
						searchable
						creatable
						getCreateLabel={(query) => `+ Create "${query}"`}
						onCreate={(query) => query}
						{...form.getInputProps("roles")}
					/>

					<Group grow>
						<Switch
							label="Active"
							description="User can log in"
							{...form.getInputProps("active", { type: "checkbox" })}
						/>
						<Switch
							label="MFA Enabled"
							description="Require two-factor authentication"
							{...form.getInputProps("mfaEnabled", { type: "checkbox" })}
						/>
					</Group>

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

	const form = useForm({
		initialValues: {
			newPassword: "",
			confirmPassword: "",
		},
		validate: {
			newPassword: (value) => {
				if (value.length < 8) return "Password must be at least 8 characters";
				if (getPasswordStrength(value).score < 2) return "Password is too weak";
				return null;
			},
			confirmPassword: (value, values) =>
				value !== values.newPassword ? "Passwords do not match" : null,
		},
	});

	const passwordStrength = getPasswordStrength(form.values.newPassword);

	const handleSubmit = async (values: typeof form.values) => {
		if (!user?.id) return;

		try {
			await resetPassword.mutateAsync({ userId: user.id, newPassword: values.newPassword });
			form.reset();
			onClose();
		} catch {
			// Error handled by hook
		}
	};

	return (
		<Modal opened={opened} onClose={onClose} title="Reset Password">
			<form onSubmit={form.onSubmit(handleSubmit)}>
				<Stack gap="md">
					<Text size="sm">
						Reset password for user: <strong>{user?.name || user?.username}</strong>
					</Text>

					<div>
						<PasswordInput
							label="New Password"
							required
							{...form.getInputProps("newPassword")}
						/>
						{form.values.newPassword && (
							<>
								<div className={classes.passwordStrength}>
									{[0, 1, 2, 3].map((i) => (
										<div
											key={i}
											className={classes.strengthBar}
											data-active={i < passwordStrength.score}
											data-strength={passwordStrength.label}
										/>
									))}
								</div>
								<Text
									className={classes.strengthLabel}
									c={
										passwordStrength.label === "weak"
											? "red"
											: passwordStrength.label === "fair"
												? "orange"
												: passwordStrength.label === "good"
													? "yellow"
													: "green"
									}
								>
									Password strength: {passwordStrength.label}
								</Text>
							</>
						)}
					</div>

					<PasswordInput
						label="Confirm Password"
						required
						{...form.getInputProps("confirmPassword")}
					/>

					<Group justify="flex-end" mt="md">
						<Button variant="light" onClick={onClose}>
							Cancel
						</Button>
						<Button type="submit" loading={resetPassword.isPending}>
							Reset Password
						</Button>
					</Group>
				</Stack>
			</form>
		</Modal>
	);
}
