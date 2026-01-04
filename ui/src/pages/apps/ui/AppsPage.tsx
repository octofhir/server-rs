import { useState, useMemo } from "react";
import { useNavigate } from "react-router-dom";
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
	Textarea,
	Select,
	Code,
} from "@mantine/core";
import { useDisclosure, useDebouncedValue } from "@mantine/hooks";
import { useForm } from "@mantine/form";
import {
	IconPlus,
	IconSearch,
	IconDotsVertical,
	IconEdit,
	IconTrash,
	IconRocket,
	IconExternalLink,
	IconEye,
	IconApi,
	IconWebhook,
} from "@tabler/icons-react";
import { useApps, useCreateApp, useUpdateApp, useDeleteApp, type AppResource } from "../lib/useApps";

const STATUS_COLORS: Record<string, string> = {
	active: "green",
	inactive: "gray",
	suspended: "fire",
};

export function AppsPage() {
	const navigate = useNavigate();
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingApp, setEditingApp] = useState<AppResource | null>(null);

	const { data, isLoading } = useApps({ search: debouncedSearch });
	const deleteApp = useDeleteApp();

	const handleView = (id: string) => {
		navigate(`/apps/${id}`);
	};

	const handleEdit = (app: AppResource) => {
		setEditingApp(app);
		open();
	};

	const handleDelete = (id: string) => {
		if (confirm("Are you sure you want to delete this application?")) {
			deleteApp.mutate(id);
		}
	};

	const handleClose = () => {
		setEditingApp(null);
		close();
	};

	const apps = data?.entry?.map((e) => e.resource) || [];

	const getAppStatus = (app: AppResource) => {
		if (app.status) return app.status;
		return app.active ? "active" : "inactive";
	};

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<Group justify="space-between">
				<div>
					<Title order={2}>API Gateway Apps</Title>
					<Text c="dimmed" size="sm">
						Group custom operations under base paths and common configuration
					</Text>
				</div>
				<Button leftSection={<IconPlus size={16} />} onClick={open}>
					Create App
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

				<Table striped highlightOnHover>
					<Table.Thead>
						<Table.Tr>
							<Table.Th>Application</Table.Th>
							<Table.Th>Endpoint</Table.Th>
							<Table.Th>Operations</Table.Th>
							<Table.Th>Status</Table.Th>
							<Table.Th style={{ width: 50 }} />
						</Table.Tr>
					</Table.Thead>
					<Table.Tbody>
						{isLoading ? (
							<Table.Tr>
								<Table.Td colSpan={5}>Loading...</Table.Td>
							</Table.Tr>
						) : apps.length === 0 ? (
							<Table.Tr>
								<Table.Td colSpan={5} style={{ textAlign: "center" }}>
									No applications found
								</Table.Td>
							</Table.Tr>
						) : (
							apps.map((app) => {
								const status = getAppStatus(app);
								return (
									<Table.Tr
										key={app.id}
										style={{ cursor: "pointer" }}
										onClick={() => app.id && handleView(app.id)}
									>
										<Table.Td>
											<Group gap="xs">
												<IconRocket size={16} color="var(--mantine-color-primary-6)" />
												<div>
													<Text size="sm" fw={500}>
														{app.name}
													</Text>
													<Text size="xs" c="dimmed" maw={300} lineClamp={1}>
														{app.description || "No description"}
													</Text>
												</div>
											</Group>
										</Table.Td>
										<Table.Td>
											{app.endpoint?.url ? (
												<Code size="xs" style={{ maxWidth: 200 }} lineClamp={1}>
													{app.endpoint.url}
												</Code>
											) : app.basePath ? (
												<Badge variant="light" radius="sm" size="sm">
													{app.basePath}
												</Badge>
											) : (
												<Text size="xs" c="dimmed">Not configured</Text>
											)}
										</Table.Td>
										<Table.Td>
											<Group gap={4}>
												{app.operations && app.operations.length > 0 && (
													<Badge size="xs" variant="light" color="primary" leftSection={<IconApi size={10} />}>
														{app.operations.length}
													</Badge>
												)}
												{app.subscriptions && app.subscriptions.length > 0 && (
													<Badge size="xs" variant="light" color="warm" leftSection={<IconWebhook size={10} />}>
														{app.subscriptions.length}
													</Badge>
												)}
												{(!app.operations || app.operations.length === 0) &&
													(!app.subscriptions || app.subscriptions.length === 0) && (
													<Text size="xs" c="dimmed">—</Text>
												)}
											</Group>
										</Table.Td>
										<Table.Td>
											<Badge
												color={STATUS_COLORS[status] ?? "gray"}
												variant="light"
												size="sm"
											>
												{status}
											</Badge>
										</Table.Td>
										<Table.Td onClick={(e) => e.stopPropagation()}>
											<Menu position="bottom-end" withinPortal>
												<Menu.Target>
													<ActionIcon variant="subtle" color="gray">
														<IconDotsVertical size={16} />
													</ActionIcon>
												</Menu.Target>
												<Menu.Dropdown>
													<Menu.Item
														leftSection={<IconEye size={14} />}
														onClick={() => app.id && handleView(app.id)}
													>
														View Details
													</Menu.Item>
													<Menu.Item
														leftSection={<IconEdit size={14} />}
														onClick={() => handleEdit(app)}
													>
														Edit JSON
													</Menu.Item>
													{app.endpoint?.url && (
														<Menu.Item
															leftSection={<IconExternalLink size={14} />}
															component="a"
															href={app.endpoint.url}
															target="_blank"
														>
															Open Endpoint
														</Menu.Item>
													)}
													<Menu.Divider />
													<Menu.Item
														leftSection={<IconTrash size={14} />}
														color="red"
														onClick={() => app.id && handleDelete(app.id)}
													>
														Delete
													</Menu.Item>
												</Menu.Dropdown>
											</Menu>
										</Table.Td>
									</Table.Tr>
								);
							})
						)}
					</Table.Tbody>
				</Table>
			</Paper>

			<AppModal
				opened={opened}
				onClose={handleClose}
				app={editingApp}
			/>
		</Stack>
	);
}

function AppModal({
	opened,
	onClose,
	app,
}: {
	opened: boolean;
	onClose: () => void;
	app: AppResource | null;
}) {
	const create = useCreateApp();
	const update = useUpdateApp();
	const isEditing = !!app;

	const form = useForm({
		initialValues: {
			name: "",
			description: "",
			endpointUrl: "",
			endpointTimeout: 30,
			secret: "",
			status: "active" as "active" | "inactive" | "suspended",
		},
		validate: {
			name: (value) => (value.length < 3 ? "Name must be at least 3 characters" : null),
			endpointUrl: (value) => {
				if (!value) return "Endpoint URL is required";
				try {
					new URL(value);
					return null;
				} catch {
					return "Must be a valid URL";
				}
			},
			secret: (value) => {
				// Secret is required for new apps, optional for editing
				if (!isEditing && !value) return "Secret is required";
				if (value && value.length < 8) return "Secret must be at least 8 characters";
				return null;
			},
		},
	});

	useMemo(() => {
		if (app) {
			form.setValues({
				name: app.name,
				description: app.description || "",
				endpointUrl: app.endpoint?.url || "",
				endpointTimeout: app.endpoint?.timeout || 30,
				secret: "", // Don't populate secret for editing
				status: app.status || (app.active ? "active" : "inactive"),
			});
		} else {
			form.reset();
		}
	}, [app]);

	const handleSubmit = async (values: typeof form.values) => {
		const payload: Partial<AppResource> = {
			resourceType: "App",
			name: values.name,
			description: values.description || undefined,
			status: values.status,
			endpoint: {
				url: values.endpointUrl,
				timeout: values.endpointTimeout,
			},
		};

		// Only include secret if provided
		if (values.secret) {
			payload.secret = values.secret;
		}

		try {
			if (isEditing && app?.id) {
				await update.mutateAsync({ ...payload, id: app.id } as AppResource);
			} else {
				await create.mutateAsync(payload);
			}
			onClose();
		} catch {
			// Handled by hook
		}
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit Application" : "Create Application"}
			size="md"
		>
			<form onSubmit={form.onSubmit(handleSubmit)}>
				<Stack gap="md">
					<TextInput
						label="App Name"
						required
						placeholder="My Application"
						{...form.getInputProps("name")}
					/>

					<Textarea
						label="Description"
						placeholder="Brief description of your application"
						{...form.getInputProps("description")}
					/>

					<TextInput
						label="Endpoint URL"
						required
						placeholder="http://backend:3000/api"
						description="Backend URL for proxying requests"
						{...form.getInputProps("endpointUrl")}
					/>

					<TextInput
						label="Timeout (seconds)"
						type="number"
						placeholder="30"
						description="Request timeout in seconds"
						{...form.getInputProps("endpointTimeout")}
					/>

					<TextInput
						label={isEditing ? "Secret (leave empty to keep current)" : "Secret"}
						required={!isEditing}
						type="password"
						placeholder="Enter app secret"
						description="Used to authenticate requests from the backend"
						{...form.getInputProps("secret")}
					/>

					<Select
						label="Status"
						data={[
							{ label: "Active", value: "active" },
							{ label: "Inactive", value: "inactive" },
							{ label: "Suspended", value: "suspended" },
						]}
						{...form.getInputProps("status")}
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
