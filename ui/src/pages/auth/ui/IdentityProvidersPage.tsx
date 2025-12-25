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
} from "@mantine/core";
import { useDisclosure, useDebouncedValue } from "@mantine/hooks";
import { useForm } from "@mantine/form";
import {
	IconPlus,
	IconSearch,
	IconDotsVertical,
	IconEdit,
	IconTrash,
	IconWorld,
} from "@tabler/icons-react";
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
				<Button leftSection={<IconPlus size={16} />} onClick={open}>
					Add Provider
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
											<IconWorld size={16} color="blue" />
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
													<IconDotsVertical size={16} />
												</ActionIcon>
											</Menu.Target>
											<Menu.Dropdown>
												<Menu.Item
													leftSection={<IconEdit size={14} />}
													onClick={() => handleEdit(idp)}
												>
													Edit
												</Menu.Item>
												<Menu.Item
													leftSection={<IconTrash size={14} />}
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

	const form = useForm({
		initialValues: {
			name: "",
			title: "",
			description: "",
			type: "oidc" as "oidc" | "oauth2" | "saml2",
			issuer: "",
			clientId: "",
			clientSecret: "",
			authorizeUrl: "",
			tokenUrl: "",
			jwksUrl: "",
			userInfoUrl: "",
			scopes: ["openid", "profile", "email"],
			active: true,
		},
		validate: {
			name: (value) => (value.length < 2 ? "Name too short" : null),
			issuer: (value) => (value.startsWith("http") ? null : "Must be a valid URL"),
			clientId: (value) => (value ? null : "Client ID required"),
		},
	});

	useMemo(() => {
		if (idp) {
			form.setValues({
				name: idp.name,
				title: idp.title || "",
				description: idp.description || "",
				type: idp.type || "oidc",
				issuer: idp.issuer,
				clientId: idp.clientId,
				clientSecret: "", // Hide secret
				authorizeUrl: idp.authorizeUrl || "",
				tokenUrl: idp.tokenUrl || "",
				jwksUrl: idp.jwksUrl || "",
				userInfoUrl: idp.userInfoUrl || "",
				scopes: idp.scopes || ["openid", "profile", "email"],
				active: idp.active,
			});
		} else {
			form.reset();
		}
	}, [idp]);

	const handleSubmit = async (values: typeof form.values) => {
		const payload: any = {
			resourceType: "IdentityProvider",
			...values,
		};

		if (isEditing && !values.clientSecret) {
			delete payload.clientSecret;
		}

		try {
			if (isEditing && idp?.id) {
				await update.mutateAsync({ ...payload, id: idp.id });
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
			title={isEditing ? "Edit Identity Provider" : "Add Identity Provider"}
			size="lg"
		>
			<form onSubmit={form.onSubmit(handleSubmit)}>
				<Stack gap="md">
					<Group grow>
						<TextInput label="Internal Name" required {...form.getInputProps("name")} />
						<TextInput label="Display Title" {...form.getInputProps("title")} />
					</Group>

					<Select
						label="Type"
						data={[
							{ label: "OpenID Connect", value: "oidc" },
							{ label: "OAuth 2.0", value: "oauth2" },
							{ label: "SAML 2.0", value: "saml2" },
						]}
						{...form.getInputProps("type")}
					/>

					<TextInput label="Issuer URL" required {...form.getInputProps("issuer")} />

					<Group grow>
						<TextInput label="Client ID" required {...form.getInputProps("clientId")} />
						<PasswordInput 
							label="Client Secret" 
							placeholder={isEditing ? "Leave blank to keep current" : ""}
							{...form.getInputProps("clientSecret")} 
						/>
					</Group>

					<Group grow>
						<TextInput label="Authorize URL" {...form.getInputProps("authorizeUrl")} />
						<TextInput label="Token URL" {...form.getInputProps("tokenUrl")} />
					</Group>

					<Group grow>
						<TextInput label="JWKS URL" {...form.getInputProps("jwksUrl")} />
						<TextInput label="User Info URL" {...form.getInputProps("userInfoUrl")} />
					</Group>

					<MultiSelect
						label="Default Scopes"
						data={form.values.scopes}
						searchable
						creatable
						getCreateLabel={(query) => `+ Add ${query}`}
						onCreate={(query) => query}
						{...form.getInputProps("scopes")}
					/>

					<Switch label="Active" {...form.getInputProps("active", { type: "checkbox" })} />

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
