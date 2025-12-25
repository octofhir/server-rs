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
	Switch,
	NumberInput,
	Textarea,
} from "@mantine/core";
import { useDisclosure, useDebouncedValue } from "@mantine/hooks";
import { useForm } from "@mantine/form";
import {
	IconPlus,
	IconSearch,
	IconDotsVertical,
	IconEdit,
	IconTrash,
	IconAppWindow,
} from "@tabler/icons-react";
import { useClients, useCreateClient, useUpdateClient, useDeleteClient, type ClientResource } from "../lib/useClients";

export function ClientsPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingClient, setEditingClient] = useState<ClientResource | null>(null);

	const { data, isLoading } = useClients({ search: debouncedSearch });
	const deleteClient = useDeleteClient();

	const handleEdit = (client: ClientResource) => {
		setEditingClient(client);
		open();
	};

	const handleDelete = (id: string) => {
		if (confirm("Are you sure you want to delete this client?")) {
			deleteClient.mutate(id);
		}
	};

	const handleClose = () => {
		setEditingClient(null);
		close();
	};

	const clients = data?.entry?.map((e) => e.resource) || [];

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<Group justify="space-between">
				<div>
					<Title order={2}>Clients</Title>
					<Text c="dimmed" size="sm">
						Manage OAuth 2.0 applications and credentials
					</Text>
				</div>
				<Button leftSection={<IconPlus size={16} />} onClick={open}>
					Register Client
				</Button>
			</Group>

			<Paper p="md" withBorder>
				<Group mb="md">
					<TextInput
						placeholder="Search by name..."
						leftSection={<IconSearch size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						style={{ flex: 1 }}
					/>
				</Group>

				<Table>
					<Table.Thead>
						<Table.Tr>
							<Table.Th>Name / Client ID</Table.Th>
							<Table.Th>Type</Table.Th>
							<Table.Th>Grant Types</Table.Th>
							<Table.Th>Status</Table.Th>
							<Table.Th style={{ width: 50 }} />
						</Table.Tr>
					</Table.Thead>
					<Table.Tbody>
						{isLoading ? (
							<Table.Tr>
								<Table.Td colSpan={5}>Loading...</Table.Td>
							</Table.Tr>
						) : clients.length === 0 ? (
							<Table.Tr>
								<Table.Td colSpan={5} style={{ textAlign: "center" }}>
									No clients found
								</Table.Td>
							</Table.Tr>
						) : (
							clients.map((client) => (
								<Table.Tr key={client.id}>
									<Table.Td>
										<Group gap="xs">
											<IconAppWindow size={16} color="gray" />
											<div>
												<Text size="sm" fw={500}>
													{client.name}
												</Text>
												<Text size="xs" c="dimmed">
													{client.clientId}
												</Text>
											</div>
										</Group>
									</Table.Td>
									<Table.Td>
										<Badge
											variant="outline"
											color={client.confidential ? "blue" : "gray"}
										>
											{client.confidential ? "Confidential" : "Public"}
										</Badge>
									</Table.Td>
									<Table.Td>
										<Group gap={4}>
											{client.grantTypes?.map((gt) => (
												<Badge key={gt} size="sm" variant="dot">
													{gt}
												</Badge>
											))}
										</Group>
									</Table.Td>
									<Table.Td>
										<Badge
											color={client.active ? "green" : "gray"}
											variant="light"
										>
											{client.active ? "Active" : "Inactive"}
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
													onClick={() => handleEdit(client)}
												>
													Edit
												</Menu.Item>
												<Menu.Item
													leftSection={<IconTrash size={14} />}
													color="red"
													onClick={() => handleDelete(client.id!)}
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

			<ClientModal
				opened={opened}
				onClose={handleClose}
				client={editingClient}
			/>
		</Stack>
	);
}

const GRANT_TYPES = [
	{ label: "Authorization Code", value: "authorization_code" },
	{ label: "Client Credentials", value: "client_credentials" },
	{ label: "Refresh Token", value: "refresh_token" },
	{ label: "Password", value: "password" },
];

function ClientModal({
	opened,
	onClose,
	client,
}: {
	opened: boolean;
	onClose: () => void;
	client: ClientResource | null;
}) {
	const create = useCreateClient();
	const update = useUpdateClient();
	const isEditing = !!client;

	const form = useForm({
		initialValues: {
			clientId: "",
			clientSecret: "",
			name: "",
			description: "",
			grantTypes: ["authorization_code", "refresh_token"] as string[],
			redirectUris: [] as string[],
			scopes: [] as string[],
			confidential: false,
			active: true,
			accessTokenLifetime: 3600,
			refreshTokenLifetime: 2592000,
			pkceRequired: true,
			allowedOrigins: [] as string[],
		},
		validate: {
			clientId: (value) => (value.length < 3 ? "Client ID must be at least 3 characters" : null),
			name: (value) => (value.length < 3 ? "Name must be at least 3 characters" : null),
			grantTypes: (value) => (value.length === 0 ? "Select at least one grant type" : null),
		},
	});

	useMemo(() => {
		if (client) {
			form.setValues({
				clientId: client.clientId,
				clientSecret: "", // Hide secret
				name: client.name,
				description: client.description || "",
				grantTypes: client.grantTypes || [],
				redirectUris: client.redirectUris || [],
				scopes: client.scopes || [],
				confidential: client.confidential,
				active: client.active,
				accessTokenLifetime: client.accessTokenLifetime || 3600,
				refreshTokenLifetime: client.refreshTokenLifetime || 2592000,
				pkceRequired: client.pkceRequired ?? true,
				allowedOrigins: client.allowedOrigins || [],
			});
		} else {
			form.reset();
		}
	}, [client]);

	const handleSubmit = async (values: typeof form.values) => {
		const payload: any = {
			resourceType: "Client",
			...values,
		};

		if (isEditing) {
			if (!values.clientSecret) {
				delete payload.clientSecret;
			}
		}

		try {
			if (isEditing && client?.id) {
				await update.mutateAsync({ ...payload, id: client.id });
			} else {
				await create.mutateAsync(payload);
			}
			onClose();
		} catch (e) {
			// Handled by hook
		}
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit OAuth Client" : "Register OAuth Client"}
			size="lg"
		>
			<form onSubmit={form.onSubmit(handleSubmit)}>
				<Stack gap="md">
					<Group grow>
						<TextInput
							label="Client ID"
							required
							{...form.getInputProps("clientId")}
							disabled={isEditing}
						/>
						<TextInput
							label="Name"
							required
							{...form.getInputProps("name")}
						/>
					</Group>

					<Textarea
						label="Description"
						{...form.getInputProps("description")}
					/>

					<Group grow>
						<PasswordInput
							label={isEditing ? "New Client Secret" : "Client Secret"}
							placeholder={isEditing ? "Leave blank to keep current" : "Required for confidential clients"}
							{...form.getInputProps("clientSecret")}
						/>
						<Stack gap={0} pt="xs">
							<Checkbox
								label="Confidential Client"
								description="Has a secret, for server-side apps"
								{...form.getInputProps("confidential", { type: "checkbox" })}
							/>
						</Stack>
					</Group>

					<MultiSelect
						label="Grant Types"
						required
						data={GRANT_TYPES}
						{...form.getInputProps("grantTypes")}
					/>

					<MultiSelect
						label="Redirect URIs"
						placeholder="Add URI and press Enter"
						data={form.values.redirectUris}
						searchable
						creatable
						getCreateLabel={(query) => `+ Add ${query}`}
						onCreate={(query) => {
							return query;
						}}
						{...form.getInputProps("redirectUris")}
					/>

					<MultiSelect
						label="Allowed Origins (CORS)"
						placeholder="Add origin and press Enter"
						data={form.values.allowedOrigins}
						searchable
						creatable
						getCreateLabel={(query) => `+ Add ${query}`}
						onCreate={(query) => {
							return query;
						}}
						{...form.getInputProps("allowedOrigins")}
					/>

					<Group grow>
						<NumberInput
							label="Access Token Lifetime (s)"
							{...form.getInputProps("accessTokenLifetime")}
						/>
						<NumberInput
							label="Refresh Token Lifetime (s)"
							{...form.getInputProps("refreshTokenLifetime")}
						/>
					</Group>

					<Group grow>
						<Switch
							label="Active"
							{...form.getInputProps("active", { type: "checkbox" })}
						/>
						<Switch
							label="Require PKCE"
							{...form.getInputProps("pkceRequired", { type: "checkbox" })}
						/>
					</Group>

					<Group justify="flex-end" mt="md">
						<Button variant="light" onClick={onClose}>
							Cancel
						</Button>
						<Button type="submit" loading={create.isPending || update.isPending}>
							{isEditing ? "Update" : "Register"}
						</Button>
					</Group>
				</Stack>
			</form>
		</Modal>
	);
}
