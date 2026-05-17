import { useState } from "react";
import {
	Stack,
	Text,
	Paper,
	Group,
	Button,
	TextInput,
	DataPreview,
	Badge,
	ActionIcon,
	Menu,
	Modal,
	Switch,
	Select,
	PasswordInput,
	MultiSelect,
} from "@/shared/ui";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { Field, Form, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import {
	Plus,
	Magnifier,
	EllipsisVertical,
	Pencil,
	TrashBin,
	Globe,
} from "@gravity-ui/icons";
import {
	getIdentityProviderStatusView,
	getIdentityProviderTypeView,
	identityProviderTypeOptions,
	type IdentityProviderResource,
	type IdentityProviderType,
} from "@/entities/identity-provider";
import { useIdentityProviders, useCreateIdentityProvider, useUpdateIdentityProvider, useDeleteIdentityProvider } from "../lib/useIdentityProviders";
import { getBundleResources } from "@/shared/api/guards";

export function IdentityProvidersPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingIdp, setEditingIdp] = useState<IdentityProviderResource | null>(null);

	const { data, isLoading } = useIdentityProviders({ search: debouncedSearch });
	const deleteIdp = useDeleteIdentityProvider();

	const handleEdit = (idp: IdentityProviderResource) => {
		setEditingIdp(idp);
		open();
	};

	const handleDelete = (id: string) => {
		if (confirm("Are you sure you want to delete this identity provider?")) {
			deleteIdp.mutate(id);
		}
	};

	const handleClose = () => {
		setEditingIdp(null);
		close();
	};

	const providers = getBundleResources<IdentityProviderResource>(data);

	return (
		<WorkspacePageLayout
			title="Identity Providers"
			description="Manage external OIDC/OAuth2 authentication providers"
			actions={
				<Button leftSection={<Plus size={16} />} onClick={open}>
					Add Provider
				</Button>
			}
			toolbar={
				<Group>
					<TextInput
						placeholder="Search by name..."
						leftSection={<Magnifier size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						style={{ flex: 1, maxWidth: 460 }}
					/>
				</Group>
			}
		>

			<Paper p="sm" withBorder>
				<DataPreview
					columns={[
						{ id: "provider", label: "Name / Issuer" },
						{ id: "type", label: "Type", width: 130 },
						{ id: "status", label: "Status", width: 110 },
						{ id: "actions", label: "", width: 48 },
					]}
					rows={
						isLoading
							? []
							: providers.map((provider) => {
									const typeView = getIdentityProviderTypeView(provider.type);
									const statusView = getIdentityProviderStatusView(provider);

									return {
										provider: (
											<Group gap="xs">
												<Globe size={16} color="blue" />
												<div>
													<Text size="sm" fw={500}>
														{provider.name}
													</Text>
													<Text size="xs" c="dimmed">
														{provider.issuer}
													</Text>
												</div>
											</Group>
										),
										type: (
											<Badge variant="outline" color={typeView.color}>
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
														onClick={() => handleEdit(provider)}
													>
														Edit
													</Menu.Item>
													<Menu.Item
														leftSection={<TrashBin size={14} />}
														color="red"
														onClick={() => provider.id && handleDelete(provider.id)}
													>
														Delete
													</Menu.Item>
												</Menu.Dropdown>
											</Menu>
										),
									};
								})
					}
					emptyText={isLoading ? "Loading providers..." : "No providers found"}
					getRowKey={(_row, index) => providers[index]?.id ?? providers[index]?.name ?? `${index}`}
				/>
			</Paper>

			<IdpModal
				opened={opened}
				onClose={handleClose}
				idp={editingIdp}
			/>
		</WorkspacePageLayout>
	);
}

interface IdpFormValues {
	name: string;
	title: string;
	description: string;
	type: IdentityProviderType;
	issuer: string;
	clientId: string;
	clientSecret: string;
	authorizeUrl: string;
	tokenUrl: string;
	jwksUrl: string;
	userInfoUrl: string;
	scopes: string[];
	active: boolean;
}

const IDP_DEFAULTS: IdpFormValues = {
	name: "",
	title: "",
	description: "",
	type: "oidc",
	issuer: "",
	clientId: "",
	clientSecret: "",
	authorizeUrl: "",
	tokenUrl: "",
	jwksUrl: "",
	userInfoUrl: "",
	scopes: ["openid", "profile", "email"],
	active: true,
};

function validateIdp(values: IdpFormValues) {
	const errors: Partial<Record<keyof IdpFormValues, string>> = {};
	if (!values.name || values.name.length < 2) errors.name = "Name too short";
	if (!values.issuer || !values.issuer.startsWith("http")) errors.issuer = "Must be a valid URL";
	if (!values.clientId) errors.clientId = "Client ID required";
	return errors;
}

function IdpModal({
	opened,
	onClose,
	idp,
}: {
	opened: boolean;
	onClose: () => void;
	idp: IdentityProviderResource | null;
}) {
	const create = useCreateIdentityProvider();
	const update = useUpdateIdentityProvider();
	const isEditing = !!idp;

	const initialValues: IdpFormValues = idp
		? {
				name: idp.name,
				title: idp.title ?? "",
				description: idp.description ?? "",
				type: idp.type ?? "oidc",
				issuer: idp.issuer,
				clientId: idp.clientId,
				clientSecret: "",
				authorizeUrl: idp.authorizeUrl ?? "",
				tokenUrl: idp.tokenUrl ?? "",
				jwksUrl: idp.jwksUrl ?? "",
				userInfoUrl: idp.userInfoUrl ?? "",
				scopes: idp.scopes ?? ["openid", "profile", "email"],
				active: idp.active,
			}
		: IDP_DEFAULTS;

	const handleSubmit = async (values: IdpFormValues) => {
		const payload: IdentityProviderResource = {
			resourceType: "IdentityProvider",
			...values,
		};
		if (isEditing && !values.clientSecret) delete payload.clientSecret;
		try {
			if (isEditing && idp?.id) {
				await update.mutateAsync({ ...payload, id: idp.id });
			} else {
				await create.mutateAsync(payload);
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
			title={isEditing ? "Edit Identity Provider" : "Add Identity Provider"}
			size="lg"
		>
			<Form<IdpFormValues>
				key={idp?.id ?? "new"}
				onSubmit={handleSubmit}
				validate={validateIdp}
				initialValues={initialValues}
				render={({ handleSubmit: submit, submitting }) => (
					<form onSubmit={submit}>
						<Stack gap="md">
							<Group grow>
								<Field<string> name="name">
									{({ input, meta }) => (
										<TextInput
											label="Internal Name"
											required
											value={input.value}
											onChange={input.onChange}
											onBlur={input.onBlur}
											error={meta.touched && meta.error ? meta.error : undefined}
										/>
									)}
								</Field>
								<Field<string> name="title">
									{({ input }) => (
										<TextInput
											label="Display Title"
											value={input.value}
											onChange={input.onChange}
										/>
									)}
								</Field>
							</Group>

							<Field<string> name="type">
								{({ input }) => (
									<Select
										label="Type"
										data={identityProviderTypeOptions}
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Field<string> name="issuer">
								{({ input, meta }) => (
									<TextInput
										label="Issuer URL"
										required
										value={input.value}
										onChange={input.onChange}
										onBlur={input.onBlur}
										error={meta.touched && meta.error ? meta.error : undefined}
									/>
								)}
							</Field>

							<Group grow>
								<Field<string> name="clientId">
									{({ input, meta }) => (
										<TextInput
											label="Client ID"
											required
											value={input.value}
											onChange={input.onChange}
											onBlur={input.onBlur}
											error={meta.touched && meta.error ? meta.error : undefined}
										/>
									)}
								</Field>
								<Field<string> name="clientSecret">
									{({ input }) => (
										<PasswordInput
											label="Client Secret"
											placeholder={isEditing ? "Leave blank to keep current" : ""}
											value={input.value}
											onChange={input.onChange}
										/>
									)}
								</Field>
							</Group>

							<Group grow>
								<Field<string> name="authorizeUrl">
									{({ input }) => (
										<TextInput label="Authorize URL" value={input.value} onChange={input.onChange} />
									)}
								</Field>
								<Field<string> name="tokenUrl">
									{({ input }) => (
										<TextInput label="Token URL" value={input.value} onChange={input.onChange} />
									)}
								</Field>
							</Group>

							<Group grow>
								<Field<string> name="jwksUrl">
									{({ input }) => (
										<TextInput label="JWKS URL" value={input.value} onChange={input.onChange} />
									)}
								</Field>
								<Field<string> name="userInfoUrl">
									{({ input }) => (
										<TextInput label="User Info URL" value={input.value} onChange={input.onChange} />
									)}
								</Field>
							</Group>

							<Field<string[]> name="scopes">
								{({ input }) => (
									<MultiSelect
										label="Default Scopes"
										data={input.value}
										searchable
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Field<boolean> name="active" type="checkbox">
								{({ input }) => (
									<Switch label="Active" checked={input.checked ?? false} onChange={input.onChange} />
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
