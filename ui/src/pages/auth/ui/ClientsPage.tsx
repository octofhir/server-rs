import { useState, useMemo } from "react";
import {
	Stack,
	Title,
	Text,
	Group,
	Table,
	Badge,
	Menu,
	PasswordInput,
	Checkbox,
	MultiSelect,
	Switch,
	NumberInput,
	Textarea,
	CopyButton,
	Tooltip,
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
	IconRefresh,
	IconCopy,
	IconCheck,
} from "@tabler/icons-react";
import { Card } from "@/shared/ui/Card/Card";
import { Modal } from "@/shared/ui/Modal/Modal";
import { Button } from "@/shared/ui/Button/Button";
import { TextInput } from "@/shared/ui/TextInput/TextInput";
import { ActionIcon } from "@/shared/ui/ActionIcon/ActionIcon";
import {
	useClients,
	useCreateClient,
	useUpdateClient,
	useDeleteClient,
	useRegenerateSecret,
	type ClientResource,
	type RegenerateSecretResponse,
} from "../lib/useClients";
import { SecretDisplayModal } from "./SecretDisplayModal";
import { DeleteClientModal } from "./DeleteClientModal";
import classes from "./ClientsPage.module.css";

export function ClientsPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingClient, setEditingClient] = useState<ClientResource | null>(null);
	const [deleteTarget, setDeleteTarget] = useState<ClientResource | null>(null);
	const [secretData, setSecretData] = useState<RegenerateSecretResponse | null>(null);
	const [isNewClientSecret, setIsNewClientSecret] = useState(false);

	const { data, isLoading } = useClients({ search: debouncedSearch });
	const deleteClient = useDeleteClient();
	const regenerateSecret = useRegenerateSecret();

	const handleEdit = (client: ClientResource) => {
		setEditingClient(client);
		open();
	};

	const handleDeleteClick = (client: ClientResource) => {
		setDeleteTarget(client);
	};

	const handleDeleteConfirm = () => {
		if (deleteTarget?.id) {
			deleteClient.mutate(deleteTarget.id, {
				onSuccess: () => setDeleteTarget(null),
			});
		}
	};

	const handleRegenerateSecret = async (client: ClientResource) => {
		try {
			const result = await regenerateSecret.mutateAsync(client.clientId);
			setSecretData(result);
			setIsNewClientSecret(false);
		} catch {
			// Error handled by hook
		}
	};

	const handleClose = () => {
		setEditingClient(null);
		close();
	};

	const handleSecretModalClose = () => {
		setSecretData(null);
	};

	const handleClientCreated = (clientId: string, secret: string) => {
		setSecretData({ clientId, clientSecret: secret });
		setIsNewClientSecret(true);
	};

	const clients = data?.entry?.map((e) => e.resource) || [];

	return (
		<Stack gap="md" className={classes.pageRoot}>
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

			<Card className={classes.tableContainer}>
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
												<div className={classes.clientIdCell}>
													<Text className={classes.clientIdText}>
														{client.clientId}
													</Text>
													<CopyButton value={client.clientId} timeout={2000}>
														{({ copied, copy }) => (
															<Tooltip
																label={copied ? "Copied!" : "Copy Client ID"}
																withArrow
																position="right"
															>
																<ActionIcon
																	variant="subtle"
																	size="xs"
																	color={copied ? "teal" : "gray"}
																	onClick={copy}
																>
																	{copied ? (
																		<IconCheck size={12} />
																	) : (
																		<IconCopy size={12} />
																	)}
																</ActionIcon>
															</Tooltip>
														)}
													</CopyButton>
												</div>
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
												{client.confidential && (
													<Menu.Item
														leftSection={<IconRefresh size={14} />}
														onClick={() => handleRegenerateSecret(client)}
													>
														Regenerate Secret
													</Menu.Item>
												)}
												<Menu.Divider />
												<Menu.Item
													leftSection={<IconTrash size={14} />}
													color="red"
													onClick={() => handleDeleteClick(client)}
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

			<ClientModal
				opened={opened}
				onClose={handleClose}
				client={editingClient}
				onSecretCreated={handleClientCreated}
			/>

			<DeleteClientModal
				opened={!!deleteTarget}
				onClose={() => setDeleteTarget(null)}
				onConfirm={handleDeleteConfirm}
				clientName={deleteTarget?.name ?? ""}
				clientId={deleteTarget?.clientId ?? ""}
				isDeleting={deleteClient.isPending}
			/>

			{secretData && (
				<SecretDisplayModal
					opened={!!secretData}
					onClose={handleSecretModalClose}
					clientId={secretData.clientId}
					clientSecret={secretData.clientSecret}
					isNewClient={isNewClientSecret}
				/>
			)}
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
	onSecretCreated,
}: {
	opened: boolean;
	onClose: () => void;
	client: ClientResource | null;
	onSecretCreated: (clientId: string, secret: string) => void;
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
			postLogoutRedirectUris: [] as string[],
			scopes: [] as string[],
			confidential: false,
			active: true,
			accessTokenLifetime: 3600,
			refreshTokenLifetime: 2592000,
			pkceRequired: true,
			allowedOrigins: [] as string[],
			jwksUri: "",
		},
		validate: {
			clientId: (value) =>
				value.length < 3 ? "Client ID must be at least 3 characters" : null,
			name: (value) =>
				value.length < 3 ? "Name must be at least 3 characters" : null,
			grantTypes: (value) =>
				value.length === 0 ? "Select at least one grant type" : null,
		},
	});

	useMemo(() => {
		if (client) {
			form.setValues({
				clientId: client.clientId,
				clientSecret: "",
				name: client.name,
				description: client.description || "",
				grantTypes: client.grantTypes || [],
				redirectUris: client.redirectUris || [],
				postLogoutRedirectUris: client.postLogoutRedirectUris || [],
				scopes: client.scopes || [],
				confidential: client.confidential,
				active: client.active,
				accessTokenLifetime: client.accessTokenLifetime || 3600,
				refreshTokenLifetime: client.refreshTokenLifetime || 2592000,
				pkceRequired: client.pkceRequired ?? true,
				allowedOrigins: client.allowedOrigins || [],
				jwksUri: client.jwksUri || "",
			});
		} else {
			form.reset();
		}
	}, [client]);

	const handleSubmit = async (values: typeof form.values) => {
		const payload: Record<string, unknown> = {
			resourceType: "Client",
			...values,
		};

		// Remove empty jwksUri
		if (!values.jwksUri) {
			delete payload.jwksUri;
		}

		if (isEditing) {
			if (!values.clientSecret) {
				delete payload.clientSecret;
			}
		}

		try {
			if (isEditing && client?.id) {
				await update.mutateAsync({ ...payload, id: client.id } as ClientResource);
				onClose();
			} else {
				const result = await create.mutateAsync(payload as Partial<ClientResource>);
				onClose();
				// If confidential client was created and we have a secret, show it
				if (values.confidential && values.clientSecret) {
					onSecretCreated(result.clientId, values.clientSecret);
				}
			}
		} catch {
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
							placeholder={
								isEditing
									? "Leave blank to keep current"
									: "Required for confidential clients"
							}
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
						onCreate={(query) => query}
						{...form.getInputProps("redirectUris")}
					/>

					<MultiSelect
						label="Post-Logout Redirect URIs"
						placeholder="Add URI and press Enter"
						description="Allowed URIs to redirect after logout"
						data={form.values.postLogoutRedirectUris}
						searchable
						creatable
						getCreateLabel={(query) => `+ Add ${query}`}
						onCreate={(query) => query}
						{...form.getInputProps("postLogoutRedirectUris")}
					/>

					<MultiSelect
						label="Allowed Origins (CORS)"
						placeholder="Add origin and press Enter"
						data={form.values.allowedOrigins}
						searchable
						creatable
						getCreateLabel={(query) => `+ Add ${query}`}
						onCreate={(query) => query}
						{...form.getInputProps("allowedOrigins")}
					/>

					<TextInput
						label="JWKS URL"
						placeholder="https://example.com/.well-known/jwks.json"
						description="For private_key_jwt authentication"
						{...form.getInputProps("jwksUri")}
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
