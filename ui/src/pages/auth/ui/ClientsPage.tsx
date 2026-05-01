import { useState, useMemo } from "react";
import {
	Stack,
	Title,
	Text,
	Group,
	DataPreview,
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
} from "@/shared/ui";
import { Field, Form, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import {
	Plus,
	Magnifier,
	EllipsisVertical,
	Pencil,
	TrashBin,
	Display,
	ArrowRotateRight,
	Copy,
	Check,
} from "@gravity-ui/icons";
import {
	getClientStatusView,
	getClientTypeView,
	oauthGrantTypeOptions,
	type ClientResource,
	type RegenerateSecretResponse,
} from "@/entities/oauth-client";
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
				<Button leftSection={<Plus size={16} />} onClick={open}>
					Register Client
				</Button>
			</Group>

			<Card className={classes.tableContainer}>
				<Group mb="md">
					<TextInput
						placeholder="Search by name..."
						leftSection={<Magnifier size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						style={{ flex: 1 }}
					/>
				</Group>

				<DataPreview
					columns={[
						{ id: "client", label: "Name / Client ID" },
						{ id: "type", label: "Type", width: 130 },
						{ id: "grantTypes", label: "Grant Types" },
						{ id: "status", label: "Status", width: 110 },
						{ id: "actions", label: "", width: 48 },
					]}
					rows={
						isLoading
							? []
							: clients.map((client) => {
									const typeView = getClientTypeView(client);
									const statusView = getClientStatusView(client);

									return {
										client: (
											<Group gap="xs">
												<Display size={16} color="gray" />
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
																			<Check size={12} />
																		) : (
																			<Copy size={12} />
																		)}
																	</ActionIcon>
																</Tooltip>
															)}
														</CopyButton>
													</div>
												</div>
											</Group>
										),
										type: (
											<Badge variant="outline" color={typeView.color}>
												{typeView.label}
											</Badge>
										),
										grantTypes: (
											<Group gap={4}>
												{client.grantTypes?.map((grantType) => (
													<Badge key={grantType} size="sm" variant="dot">
														{grantType}
													</Badge>
												))}
											</Group>
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
														onClick={() => handleEdit(client)}
													>
														Edit
													</Menu.Item>
													{client.confidential && (
														<Menu.Item
															leftSection={<ArrowRotateRight size={14} />}
															onClick={() => handleRegenerateSecret(client)}
														>
															Regenerate Secret
														</Menu.Item>
													)}
													<Menu.Divider />
													<Menu.Item
														leftSection={<TrashBin size={14} />}
														color="red"
														onClick={() => handleDeleteClick(client)}
													>
														Delete
													</Menu.Item>
												</Menu.Dropdown>
											</Menu>
										),
									};
								})
					}
					emptyText={isLoading ? "Loading clients..." : "No clients found"}
					getRowKey={(_row, index) => clients[index]?.id ?? clients[index]?.clientId ?? `${index}`}
				/>
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

	const initialValues: ClientFormValues = client
		? {
				clientId: client.clientId,
				clientSecret: "",
				name: client.name,
				description: client.description ?? "",
				grantTypes: client.grantTypes ?? [],
				redirectUris: client.redirectUris ?? [],
				postLogoutRedirectUris: client.postLogoutRedirectUris ?? [],
				scopes: client.scopes ?? [],
				confidential: client.confidential,
				active: client.active,
				accessTokenLifetime: client.accessTokenLifetime ?? 3600,
				refreshTokenLifetime: client.refreshTokenLifetime ?? 2592000,
				pkceRequired: client.pkceRequired ?? true,
				allowedOrigins: client.allowedOrigins ?? [],
				jwksUri: client.jwksUri ?? "",
			}
		: CLIENT_DEFAULTS;

	const handleSubmit = async (values: ClientFormValues) => {
		const payload: Record<string, unknown> = {
			resourceType: "Client",
			...values,
		};
		if (!values.jwksUri) delete payload.jwksUri;
		if (isEditing && !values.clientSecret) delete payload.clientSecret;
		try {
			if (isEditing && client?.id) {
				await update.mutateAsync({ ...payload, id: client.id } as ClientResource);
				onClose();
			} else {
				const result = await create.mutateAsync(payload as Partial<ClientResource>);
				onClose();
				if (values.confidential && values.clientSecret) {
					onSecretCreated(result.clientId, values.clientSecret);
				}
			}
		} catch {
			/* surfaced by mutation */
		}
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit OAuth Client" : "Register OAuth Client"}
			size="lg"
		>
			<Form<ClientFormValues>
				key={client?.id ?? "new"}
				onSubmit={handleSubmit}
				validate={validateClientForm}
				initialValues={initialValues}
				render={({ handleSubmit: submit, submitting }) => (
					<form onSubmit={submit}>
						<Stack gap="md">
							<Group grow>
								<Field<string> name="clientId">
									{({ input, meta }) => (
										<TextInput
											label="Client ID"
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
									{({ input, meta }) => (
										<TextInput
											label="Name"
											required
											value={input.value}
											onChange={input.onChange}
											onBlur={input.onBlur}
											error={meta.touched && meta.error ? meta.error : undefined}
										/>
									)}
								</Field>
							</Group>

							<Field<string> name="description">
								{({ input }) => (
									<Textarea label="Description" value={input.value} onChange={input.onChange} />
								)}
							</Field>

							<Group grow>
								<Field<string> name="clientSecret">
									{({ input }) => (
										<PasswordInput
											label={isEditing ? "New Client Secret" : "Client Secret"}
											placeholder={isEditing ? "Leave blank to keep current" : "Required for confidential clients"}
											value={input.value}
											onChange={input.onChange}
										/>
									)}
								</Field>
								<Stack gap={0} pt="xs">
									<Field<boolean> name="confidential" type="checkbox">
										{({ input }) => (
											<Checkbox
												label="Confidential Client"
												description="Has a secret, for server-side apps"
												checked={input.checked ?? false}
												onChange={input.onChange}
											/>
										)}
									</Field>
								</Stack>
							</Group>

							<Field<string[]> name="grantTypes">
								{({ input, meta }) => (
									<MultiSelect
										label="Grant Types"
										required
										data={oauthGrantTypeOptions}
										value={input.value}
										onChange={input.onChange}
										error={meta.touched && meta.error ? meta.error : undefined}
									/>
								)}
							</Field>

							<Field<string[]> name="redirectUris">
								{({ input }) => (
									<MultiSelect
										label="Redirect URIs"
										placeholder="Add URI and press Enter"
										data={input.value}
										searchable
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Field<string[]> name="postLogoutRedirectUris">
								{({ input }) => (
									<MultiSelect
										label="Post-Logout Redirect URIs"
										placeholder="Add URI and press Enter"
										description="Allowed URIs to redirect after logout"
										data={input.value}
										searchable
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Field<string[]> name="allowedOrigins">
								{({ input }) => (
									<MultiSelect
										label="Allowed Origins (CORS)"
										placeholder="Add origin and press Enter"
										data={input.value}
										searchable
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Field<string> name="jwksUri">
								{({ input }) => (
									<TextInput
										label="JWKS URL"
										placeholder="https://example.com/.well-known/jwks.json"
										description="For private_key_jwt authentication"
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Group grow>
								<Field<number> name="accessTokenLifetime">
									{({ input }) => (
										<NumberInput
											label="Access Token Lifetime (s)"
											value={input.value}
											onChange={input.onChange}
										/>
									)}
								</Field>
								<Field<number> name="refreshTokenLifetime">
									{({ input }) => (
										<NumberInput
											label="Refresh Token Lifetime (s)"
											value={input.value}
											onChange={input.onChange}
										/>
									)}
								</Field>
							</Group>

							<Group grow>
								<Field<boolean> name="active" type="checkbox">
									{({ input }) => (
										<Switch label="Active" checked={input.checked ?? false} onChange={input.onChange} />
									)}
								</Field>
								<Field<boolean> name="pkceRequired" type="checkbox">
									{({ input }) => (
										<Switch label="Require PKCE" checked={input.checked ?? false} onChange={input.onChange} />
									)}
								</Field>
							</Group>

							<Group justify="flex-end" mt="md">
								<Button variant="light" onClick={onClose} type="button">
									Cancel
								</Button>
								<Button type="submit" loading={submitting || create.isPending || update.isPending}>
									{isEditing ? "Update" : "Register"}
								</Button>
							</Group>
						</Stack>
					</form>
				)}
			/>
		</Modal>
	);
}

interface ClientFormValues {
	clientId: string;
	clientSecret: string;
	name: string;
	description: string;
	grantTypes: string[];
	redirectUris: string[];
	postLogoutRedirectUris: string[];
	scopes: string[];
	confidential: boolean;
	active: boolean;
	accessTokenLifetime: number;
	refreshTokenLifetime: number;
	pkceRequired: boolean;
	allowedOrigins: string[];
	jwksUri: string;
}

const CLIENT_DEFAULTS: ClientFormValues = {
	clientId: "",
	clientSecret: "",
	name: "",
	description: "",
	grantTypes: ["authorization_code", "refresh_token"],
	redirectUris: [],
	postLogoutRedirectUris: [],
	scopes: [],
	confidential: false,
	active: true,
	accessTokenLifetime: 3600,
	refreshTokenLifetime: 2592000,
	pkceRequired: true,
	allowedOrigins: [],
	jwksUri: "",
};

function validateClientForm(values: ClientFormValues) {
	const errors: Partial<Record<keyof ClientFormValues, string>> = {};
	if (!values.clientId || values.clientId.length < 3) errors.clientId = "Client ID must be at least 3 characters";
	if (!values.name || values.name.length < 3) errors.name = "Name must be at least 3 characters";
	if (!values.grantTypes || values.grantTypes.length === 0)
		errors.grantTypes = "Select at least one grant type";
	return errors;
}
