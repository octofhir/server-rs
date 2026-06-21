import { ActionIcon, Button, Card, Field, Form, Modal, TextInput, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import { useState } from "react";
import type { ReactNode } from "react";
import {
	Text,
	DataPreview,
	Badge,
	EmptyState,
	PasswordInput,
	Checkbox,
	MultiSelect,
	Skeleton,
	Switch,
	NumberInput,
	Textarea,
	CopyButton,
	Tooltip,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { DropdownMenu } from "@octofhir/ui-kit";
import { Plus, Search as Magnifier, EllipsisVertical, Pencil, Trash2 as TrashBin, Monitor as Display, RotateCw as ArrowRotateRight, Copy, Check } from "lucide-react";
import {
	getClientStatusView,
	getClientTypeView,
	oauthGrantTypeOptions,
	type ClientResource,
	type RegenerateSecretResponse,
} from "@/entities/oauth-client";
import {
	useClients,
	useCreateClient,
	useUpdateClient,
	useDeleteClient,
	useRegenerateSecret,
} from "../lib/useClients";
import { getBundleResources } from "@/shared/api/guards";
import { SecretDisplayModal } from "./SecretDisplayModal";
import { DeleteClientModal } from "./DeleteClientModal";
import classes from "./ClientsPage.module.css";

/** Wraps a field with an adjacent helper line (Gravity inputs have no `description`). */
function FieldWithHint({ hint, children }: { hint: ReactNode; children: ReactNode }) {
	return (
		<div className={classes.fieldWithHint}>
			{children}
			<Text variant="caption-2" color="secondary">
				{hint}
			</Text>
		</div>
	);
}

export function ClientsPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingClient, setEditingClient] = useState<ClientResource | null>(null);
	const [deleteTarget, setDeleteTarget] = useState<ClientResource | null>(null);
	const [secretData, setSecretData] = useState<RegenerateSecretResponse | null>(null);
	const [isNewClientSecret, setIsNewClientSecret] = useState(false);

	const { data, isLoading, isError, error, refetch } = useClients({ search: debouncedSearch });
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

	const clients = getBundleResources<ClientResource>(data);
	const isFiltered = debouncedSearch.length > 0;

	return (
		<WorkspacePageLayout
			title="Clients"
			description="Manage OAuth 2.0 applications and credentials"
			actions={
				<Button variant="filled" onClick={open}>
					<Button.Icon>
						<Plus width={16} />
					</Button.Icon>
					Register Client
				</Button>
			}
			toolbar={
				<div className={classes.toolbar}>
					<TextInput
						aria-label="Search clients by name"
						placeholder="Search by name..."
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
						title="Failed to load clients"
						description={error instanceof Error ? error.message : "Something went wrong while loading OAuth clients."}
						actions={[
							<Button key="retry" variant="filled" onClick={() => refetch()}>
								Retry
							</Button>,
						]}
					/>
				) : clients.length === 0 ? (
					<EmptyState
						title={isFiltered ? "No matching clients" : "No OAuth clients yet"}
						description={
							isFiltered
								? "No clients match your search. Try a different term."
								: "Register an OAuth 2.0 application to issue tokens and integrate with the server."
						}
						actions={
							isFiltered
								? [
										<Button key="clear" variant="outline" onClick={() => setSearch("")}>
											Clear filters
										</Button>,
									]
								: [
										<Button key="create" variant="filled" onClick={open}>
											Register Client
										</Button>,
									]
						}
					/>
				) : (
					<DataPreview
						columns={[
							{ id: "client", label: "Name / Client ID" },
							{ id: "type", label: "Type", width: 130 },
							{ id: "grantTypes", label: "Grant Types" },
							{ id: "status", label: "Status", width: 110 },
							{ id: "actions", label: "", width: 48 },
						]}
						rows={clients.map((client) => {
							const typeView = getClientTypeView(client);
							const statusView = getClientStatusView(client);

							return {
								client: (
									<div className={classes.clientCell}>
										<Display width={16} height={16} className={classes.clientIcon} aria-hidden="true" />
										<div className={classes.clientText}>
											<Text variant="body-2" className={classes.clientName}>
												<strong>{client.name}</strong>
											</Text>
											<div className={classes.clientIdCell}>
												<Text className={classes.clientIdText}>{client.clientId}</Text>
												<CopyButton value={client.clientId} timeout={2000}>
													{({ copied, copy }) => (
														<Tooltip content={copied ? "Copied!" : "Copy Client ID"} placement="right">
															<ActionIcon
																variant="subtle"
																size="xs"
																aria-label="Copy Client ID"
																onClick={copy}
															>
																{copied ? (
																	<Check width={12} height={12} aria-hidden="true" />
																) : (
																	<Copy width={12} height={12} aria-hidden="true" />
																)}
															</ActionIcon>
														</Tooltip>
													)}
												</CopyButton>
											</div>
										</div>
									</div>
								),
								type: <Badge color={typeView.color}>{typeView.label}</Badge>,
								grantTypes: (
									<div className={classes.badgeList}>
										{client.grantTypes?.map((grantType) => (
											<Badge key={grantType} size="sm">
												{grantType}
											</Badge>
										))}
									</div>
								),
								status: <Badge color={statusView.color}>{statusView.label}</Badge>,
								actions: (
									<DropdownMenu
										size="sm"
										icon={<EllipsisVertical width={16} />}
										defaultSwitcherProps={{
											variant: "subtle",
											size: "sm",
											"aria-label": "Client actions",
											"aria-haspopup": "menu",
										}}
										popupProps={{ placement: "bottom-end" }}
										items={[
											{
												text: "Edit",
												iconStart: <Pencil width={14} />,
												action: () => handleEdit(client),
											},
											...(client.confidential
												? [
														{
															text: "Regenerate Secret",
															iconStart: <ArrowRotateRight width={14} />,
															action: () => handleRegenerateSecret(client),
														},
													]
												: []),
											[
												{
													text: "Delete",
													iconStart: <TrashBin width={14} />,
													theme: "danger" as const,
													action: () => handleDeleteClick(client),
												},
											],
										]}
									/>
								),
							};
						})}
						getRowKey={(_row, index) => clients[index]?.id ?? clients[index]?.clientId ?? `${index}`}
					/>
				)}
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
		</WorkspacePageLayout>
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
		const payload: Partial<ClientResource> = {
			resourceType: "Client",
			...values,
		};
		if (!values.jwksUri) delete payload.jwksUri;
		if (isEditing && !values.clientSecret) delete payload.clientSecret;
		try {
			if (isEditing && client?.id) {
				await update.mutateAsync({
					...client,
					...payload,
					id: client.id,
					resourceType: "Client",
				});
				onClose();
			} else {
				const result = await create.mutateAsync(payload);
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
						<div className={classes.clientForm}>
							<div className={classes.formGrid}>
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
							</div>

							<Field<string> name="description">
								{({ input }) => (
									<Textarea label="Description" value={input.value} onChange={input.onChange} />
								)}
							</Field>

							<div className={classes.formGrid}>
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
								<div className={classes.checkboxField}>
									<Field<boolean> name="confidential" type="checkbox">
										{({ input }) => (
											<div className={classes.fieldWithHint}>
												<Checkbox
													content="Confidential Client"
													checked={input.checked ?? false}
													onChange={input.onChange}
												/>
												<Text variant="caption-2" color="secondary">
													Has a secret, for server-side apps
												</Text>
											</div>
										)}
									</Field>
								</div>
							</div>

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
									<FieldWithHint hint="Allowed URIs to redirect after logout">
										<MultiSelect
											label="Post-Logout Redirect URIs"
											placeholder="Add URI and press Enter"
											data={input.value}
											searchable
											value={input.value}
											onChange={input.onChange}
										/>
									</FieldWithHint>
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
									<FieldWithHint hint="For private_key_jwt authentication">
										<TextInput
											label="JWKS URL"
											placeholder="https://example.com/.well-known/jwks.json"
											value={input.value}
											onChange={input.onChange}
										/>
									</FieldWithHint>
								)}
							</Field>

							<div className={classes.formGrid}>
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
							</div>

							<div className={classes.switchGrid}>
								<Field<boolean> name="active" type="checkbox">
									{({ input }) => (
										<Switch content="Active" checked={input.checked ?? false} onUpdate={input.onChange} />
									)}
								</Field>
								<Field<boolean> name="pkceRequired" type="checkbox">
									{({ input }) => (
										<Switch content="Require PKCE" checked={input.checked ?? false} onUpdate={input.onChange} />
									)}
								</Field>
							</div>

							<div className={classes.formActions}>
								<Button variant="subtle" onClick={onClose} type="button">
									Cancel
								</Button>
								<Button variant="filled" type="submit" loading={submitting || create.isPending || update.isPending}>
									{isEditing ? "Update" : "Register"}
								</Button>
							</div>
						</div>
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
