import { useState, useMemo } from "react";
import {
	Stack,
	Title,
	Text,
	Paper,
	Group,
	Button,
	TextInput,
	Table,
	Badge,
	ActionIcon,
	Menu,
	Modal,
	PasswordInput,
	Checkbox,
	MultiSelect,
} from "@mantine/core";
import { useDisclosure, useDebouncedValue } from "@mantine/hooks";
import { useForm } from "@mantine/form";
import {
	IconPlus,
	IconSearch,
	IconDotsVertical,
	IconEdit,
	IconTrash,
	IconUser,
} from "@tabler/icons-react";
import { useUsers, useCreateUser, useUpdateUser, useDeleteUser } from "../lib/useUsers";
import type { UserResource } from "@/shared/api/types";

export function UsersPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingUser, setEditingUser] = useState<UserResource | null>(null);

	const { data, isLoading } = useUsers({ search: debouncedSearch });
	const deleteUser = useDeleteUser();

	const handleEdit = (user: UserResource) => {
		setEditingUser(user);
		open();
	};

	const handleDelete = (id: string) => {
		if (confirm("Are you sure you want to delete this user?")) {
			deleteUser.mutate(id);
		}
	};

	const handleClose = () => {
		setEditingUser(null);
		close();
	};

	const users = data?.entry?.map((e) => e.resource) || [];

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<Group justify="space-between">
				<div>
					<Title order={2}>Users</Title>
					<Text c="dimmed" size="sm">
						Manage user accounts and credentials
					</Text>
				</div>
				<Button leftSection={<IconPlus size={16} />} onClick={open}>
					Create User
				</Button>
			</Group>

			<Paper p="md" withBorder>
				<Group mb="md">
					<TextInput
						placeholder="Search users..."
						leftSection={<IconSearch size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						style={{ flex: 1 }}
					/>
				</Group>

				<Table>
					<Table.Thead>
						<Table.Tr>
							<Table.Th>Username</Table.Th>
							<Table.Th>Email</Table.Th>
							<Table.Th>Roles</Table.Th>
							<Table.Th>Status</Table.Th>
							<Table.Th style={{ width: 50 }} />
						</Table.Tr>
					</Table.Thead>
					<Table.Tbody>
						{isLoading ? (
							<Table.Tr>
								<Table.Td colSpan={5}>Loading...</Table.Td>
							</Table.Tr>
						) : users.length === 0 ? (
							<Table.Tr>
								<Table.Td colSpan={5} style={{ textAlign: "center" }}>
									No users found
								</Table.Td>
							</Table.Tr>
						) : (
							users.map((user) => (
								<Table.Tr key={user.id}>
									<Table.Td>
										<Group gap="xs">
											<IconUser size={16} color="gray" />
											<Text size="sm" fw={500}>
												{user.username}
											</Text>
										</Group>
									</Table.Td>
									<Table.Td>{user.email || "-"}</Table.Td>
									<Table.Td>
										<Group gap={4}>
											{user.roles?.map((role) => (
												<Badge key={role} size="sm" variant="dot">
													{role}
												</Badge>
											))}
										</Group>
									</Table.Td>
									<Table.Td>
										<Badge
											color={user.active ? "green" : "gray"}
											variant="light"
										>
											{user.active ? "Active" : "Inactive"}
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
													onClick={() => handleEdit(user)}
												>
													Edit
												</Menu.Item>
												<Menu.Item
													leftSection={<IconTrash size={14} />}
													color="red"
													onClick={() => handleDelete(user.id!)}
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
			</Paper>

			<UserModal
				opened={opened}
				onClose={handleClose}
				user={editingUser}
			/>
		</Stack>
	);
}

function UserModal({
	opened,
	onClose,
	user,
}: {
	opened: boolean;
	onClose: () => void;
	user: UserResource | null;
}) {
	const create = useCreateUser();
	const update = useUpdateUser();
	const isEditing = !!user;

	const form = useForm({
		initialValues: {
			username: "",
			email: "",
			password: "",
			active: true,
			roles: [] as string[],
		},
		validate: {
			username: (value) => (value.length < 3 ? "Username must be at least 3 characters" : null),
			email: (value) => (value && !/^\S+@\S+$/.test(value) ? "Invalid email" : null),
			password: (value) => (!isEditing && value.length < 6 ? "Password must be at least 6 characters" : null),
		},
	});

	// Update form when user changes
	useMemo(() => {
		if (user) {
			form.setValues({
				username: user.username,
				email: user.email || "",
				password: "", // Don't show password
				active: user.active,
				roles: user.roles || [],
			});
		} else {
			form.reset();
		}
	}, [user]);

	const handleSubmit = async (values: typeof form.values) => {
		const userData: any = {
			resourceType: "User",
			username: values.username,
			email: values.email || undefined,
			active: values.active,
			roles: values.roles,
		};

		if (values.password) {
			userData.password = values.password;
		}

		try {
			if (isEditing && user?.id) {
				await update.mutateAsync({ ...userData, id: user.id });
			} else {
				await create.mutateAsync(userData);
			}
			onClose();
		} catch (e) {
			// Error handled by mutation hook
		}
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit User" : "Create User"}
		>
			<form onSubmit={form.onSubmit(handleSubmit)}>
				<Stack gap="md">
					<TextInput
						label="Username"
						required
						{...form.getInputProps("username")}
						disabled={isEditing} // Often username cannot be changed
					/>
					<TextInput
						label="Email"
						{...form.getInputProps("email")}
					/>
					<PasswordInput
						label={isEditing ? "New Password" : "Password"}
						placeholder={isEditing ? "Leave blank to keep current" : ""}
						required={!isEditing}
						{...form.getInputProps("password")}
					/>
					<MultiSelect
						label="Roles"
						data={["admin", "practitioner", "patient"]}
						searchable
						creatable
						getCreateLabel={(query) => `+ Create ${query}`}
						onCreate={(query) => {
							return query;
						}}
						{...form.getInputProps("roles")}
					/>
					<Checkbox
						label="Active"
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
