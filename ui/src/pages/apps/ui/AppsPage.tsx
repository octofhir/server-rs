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
	Checkbox,
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
} from "@tabler/icons-react";
import { useApps, useCreateApp, useUpdateApp, useDeleteApp, type AppResource } from "../lib/useApps";

export function AppsPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingApp, setEditingApp] = useState<AppResource | null>(null);

	const { data, isLoading } = useApps({ search: debouncedSearch });
	const deleteApp = useDeleteApp();

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

				<Table>
					<Table.Thead>
						<Table.Tr>
							<Table.Th>Application</Table.Th>
							<Table.Th>Base Path</Table.Th>
							<Table.Th>Auth</Table.Th>
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
							apps.map((app) => (
								<Table.Tr key={app.id}>
									<Table.Td>
										<Group gap="xs">
											<IconRocket size={16} color="blue" />
											<div>
												<Text size="sm" fw={500}>
													{app.name}
												</Text>
												<Text size="xs" c="dimmed" maw={300} truncate>
													{app.description || "No description"}
												</Text>
											</div>
										</Group>
									</Table.Td>
									<Table.Td>
										<Badge variant="light" radius="sm">
											{app.basePath}
										</Badge>
									</Table.Td>
									<Table.Td>
										{app.authentication?.required ? (
											<Badge size="sm" color="violet">
												{app.authentication.type}
											</Badge>
										) : (
											<Badge size="sm" color="gray" variant="outline">
												Public
											</Badge>
										)}
									</Table.Td>
									<Table.Td>
										<Badge
											color={app.active ? "green" : "gray"}
											variant="light"
										>
											{app.active ? "Active" : "Inactive"}
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
													onClick={() => handleEdit(app)}
												>
													Edit
												</Menu.Item>
												<Menu.Item
													leftSection={<IconExternalLink size={14} />}
													component="a"
													href={`${window.location.origin}${app.basePath}`}
													target="_blank"
												>
													Visit Base Path
												</Menu.Item>
												<Menu.Divider />
												<Menu.Item
													leftSection={<IconTrash size={14} />}
													color="red"
													onClick={() => handleDelete(app.id!)}
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
			basePath: "/api/v1/",
			active: true,
			authRequired: false,
			authType: "bearer",
		},
		validate: {
			name: (value) => (value.length < 3 ? "Name must be at least 3 characters" : null),
			basePath: (value) => {
				if (!value.startsWith("/")) return "Path must start with /";
				if (!value.endsWith("/")) return "Path must end with /";
				return null;
			},
		},
	});

	useMemo(() => {
		if (app) {
			form.setValues({
				name: app.name,
				description: app.description || "",
				basePath: app.basePath,
				active: app.active,
				authRequired: app.authentication?.required || false,
				authType: app.authentication?.type || "bearer",
			});
		} else {
			form.reset();
		}
	}, [app]);

	const handleSubmit = async (values: typeof form.values) => {
		const payload: any = {
			resourceType: "App",
			name: values.name,
			description: values.description,
			basePath: values.basePath,
			active: values.active,
			authentication: {
				type: values.authType,
				required: values.authRequired,
			},
		};

		try {
			if (isEditing && app?.id) {
				await update.mutateAsync({ ...payload, id: app.id });
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
			title={isEditing ? "Edit Application" : "Create Application"}
			size="md"
		>
			<form onSubmit={form.onSubmit(handleSubmit)}>
				<Stack gap="md">
					<TextInput
						label="App Name"
						required
						{...form.getInputProps("name")}
					/>

					<Textarea
						label="Description"
						{...form.getInputProps("description")}
					/>

					<TextInput
						label="Base Path"
						required
						placeholder="/api/my-app/"
						{...form.getInputProps("basePath")}
					/>

					<Group grow>
						<Checkbox
							label="Authentication Required"
							{...form.getInputProps("authRequired", { type: "checkbox" })}
						/>
						<Switch
							label="Active"
							{...form.getInputProps("active", { type: "checkbox" })}
						/>
					</Group>

					{form.values.authRequired && (
						<Select
							label="Auth Type"
							data={[
								{ label: "Bearer Token", value: "bearer" },
								{ label: "API Key", value: "apiKey" },
								{ label: "Basic Auth", value: "basic" },
							]}
							{...form.getInputProps("authType")}
						/>
					)}

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
