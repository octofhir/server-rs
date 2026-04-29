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
	Switch,
	Textarea,
	Select,
	PasswordInput,
	MultiSelect,
} from "@/shared/ui";
import { Field, Form, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import {
	Plus,
	Magnifier,
	EllipsisVertical,
	Pencil,
	TrashBin,
	Globe,
} from "@gravity-ui/icons";
import { useIdentityProviders, useCreateIdentityProvider, useUpdateIdentityProvider, useDeleteIdentityProvider, type IdentityProviderResource } from "../lib/useIdentityProviders";

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

	const providers = data?.entry?.map((e) => e.resource) || [];

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<Group justify="space-between">
				<div>
					<Title order={2}>Identity Providers</Title>
					<Text c="dimmed" size="sm">
						Manage external OIDC/OAuth2 authentication providers
					</Text>
				</div>
				<Button leftSection={<Plus size={16} />} onClick={open}>
					Add Provider
				</Button>
			</Group>

			<Paper p="md" withBorder>
				<Group mb="md">
					<TextInput
						placeholder="Search by name..."
						leftSection={<Magnifier size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						style={{ flex: 1 }}
					/>
				</Group>

				<Table>
					<Table.Thead>
						<Table.Tr>
							<Table.Th>Name / Issuer</Table.Th>
							<Table.Th>Type</Table.Th>
							<Table.Th>Status</Table.Th>
							<Table.Th style={{ width: 50 }} />
						</Table.Tr>
					</Table.Thead>
					<Table.Tbody>
						{isLoading ? (
							<Table.Tr>
								<Table.Td colSpan={4}>Loading...</Table.Td>
							</Table.Tr>
						) : providers.length === 0 ? (
							<Table.Tr>
								<Table.Td colSpan={4} style={{ textAlign: "center" }}>
									No providers found
								</Table.Td>
							</Table.Tr>
						) : (
							providers.map((idp) => (
								<Table.Tr key={idp.id}>
									<Table.Td>
										<Group gap="xs">
											<Globe size={16} color="blue" />
											<div>
												<Text size="sm" fw={500}>
													{idp.name}
												</Text>
												<Text size="xs" c="dimmed">
													{idp.issuer}
												</Text>
											</div>
										</Group>
									</Table.Td>
									<Table.Td>
										<Badge variant="outline">{idp.type?.toUpperCase()}</Badge>
									</Table.Td>
									<Table.Td>
										<Badge
											color={idp.active ? "green" : "gray"}
											variant="light"
										>
											{idp.active ? "Active" : "Inactive"}
										</Badge>
									</Table.Td>
									<Table.Td>
										<Menu position="bottom-end" withinPortal>
											<Menu.Target>
												<ActionIcon variant="subtle" color="gray">
													<EllipsisVertical size={16} />
												</ActionIcon>
											</Menu.Target>
											<Menu.Dropdown>
												<Menu.Item
													leftSection={<Pencil size={14} />}
													onClick={() => handleEdit(idp)}
												>
													Edit
												</Menu.Item>
												<Menu.Item
													leftSection={<TrashBin size={14} />}
													color="red"
													onClick={() => handleDelete(idp.id!)}
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

			<IdpModal
				opened={opened}
				onClose={handleClose}
				idp={editingIdp}
			/>
		</Stack>
	);
}

interface IdpFormValues {
	name: string;
	title: string;
	description: string;
	type: "oidc" | "oauth2" | "saml2";
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
		const payload: Record<string, unknown> = {
			resourceType: "IdentityProvider",
			...values,
		};
		if (isEditing && !values.clientSecret) delete payload.clientSecret;
		try {
			if (isEditing && idp?.id) {
				await update.mutateAsync({ ...payload, id: idp.id } as IdentityProviderResource);
			} else {
				await create.mutateAsync(payload as IdentityProviderResource);
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
										data={[
											{ label: "OpenID Connect", value: "oidc" },
											{ label: "OAuth 2.0", value: "oauth2" },
											{ label: "SAML 2.0", value: "saml2" },
										]}
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
