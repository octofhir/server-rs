import { useState } from "react";
import { useNavigate } from "react-router-dom";
import {
	Box,
	Flex,
	Text,
	Button,
	TextInput,
	NumberInput,
	DataPreview,
	Badge,
	ActionIcon,
	Menu,
	Modal,
	Textarea,
	Select,
	Code,
	Anchor,
} from "@/shared/ui";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { Field, Form, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
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
} from "@octofhir/ui-kit";
import {
	getAppEndpointDisplay,
	getAppStatusView,
	type AppResource,
} from "@/entities/api-app";
import { useApps, useCreateApp, useUpdateApp, useDeleteApp } from "../lib/useApps";
import { getBundleResources } from "@/shared/api/guards";
import classes from "./AppsPage.module.css";

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

	const apps = getBundleResources<AppResource>(data);

	return (
		<WorkspacePageLayout
			title="API Gateway Apps"
			description="Group custom operations under base paths and common configuration"
			actions={
				<Button leftSection={<IconPlus size={16} />} onClick={open}>
					Create App
				</Button>
			}
			toolbar={
				<Flex className={classes.toolbar}>
					<TextInput
						placeholder="Search by name..."
						leftSection={<IconSearch size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						className={classes.searchInput}
					/>
				</Flex>
			}
			maxWidth={1280}
		>

			<Box className={classes.tablePanel}>
				<DataPreview
					columns={[
						{ id: "application", label: "Application" },
						{ id: "endpoint", label: "Endpoint", width: 240 },
						{ id: "operations", label: "Operations", width: 140 },
						{ id: "status", label: "Status", width: 110 },
						{ id: "actions", label: "", width: 48 },
					]}
					rows={
						isLoading
							? []
							: apps.map((app) => {
									const statusView = getAppStatusView(app);
									const endpoint = getAppEndpointDisplay(app);

									return {
										application: (
											<Flex gap="2" alignItems="center">
												<IconRocket size={16} color="var(--octo-brand-primary-active)" />
												<Box className={classes.appSummary}>
													<Anchor size="sm" onClick={() => app.id && handleView(app.id)}>
														{app.name}
													</Anchor>
													<Text size="xs" c="dimmed" className={classes.truncateText}>
														{app.description || "No description"}
													</Text>
												</Box>
											</Flex>
										),
										endpoint:
											app.endpoint?.url || app.basePath ? (
												<Code size="xs" className={classes.endpointCode}>
													{endpoint}
												</Code>
											) : (
												<Text size="xs" c="dimmed">
													{endpoint}
												</Text>
											),
										operations: (
											<Flex gap="1" wrap="wrap">
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
														<Text size="xs" c="dimmed">-</Text>
													)}
											</Flex>
										),
										status: (
											<Badge color={statusView.color} variant="light" size="sm">
												{statusView.status}
											</Badge>
										),
										actions: (
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
										),
									};
								})
					}
					emptyText={isLoading ? "Loading applications..." : "No applications found"}
					getRowKey={(_row, index) => apps[index]?.id ?? `${index}`}
				/>
			</Box>

			<AppModal
				opened={opened}
				onClose={handleClose}
				app={editingApp}
			/>
		</WorkspacePageLayout>
	);
}

interface AppFormValues {
	name: string;
	description: string;
	endpointUrl: string;
	endpointTimeout: number;
	secret: string;
	status: "active" | "inactive" | "suspended";
}

const APP_DEFAULTS: AppFormValues = {
	name: "",
	description: "",
	endpointUrl: "",
	endpointTimeout: 30,
	secret: "",
	status: "active",
};

function makeAppValidator(isEditing: boolean) {
	return (values: AppFormValues) => {
		const errors: Partial<Record<keyof AppFormValues, string>> = {};
		if (!values.name || values.name.length < 3) errors.name = "Name must be at least 3 characters";
		if (!values.endpointUrl) {
			errors.endpointUrl = "Endpoint URL is required";
		} else {
			try {
				new URL(values.endpointUrl);
			} catch {
				errors.endpointUrl = "Must be a valid URL";
			}
		}
		if (!isEditing && !values.secret) errors.secret = "Secret is required";
		else if (values.secret && values.secret.length < 8)
			errors.secret = "Secret must be at least 8 characters";
		return errors;
	};
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

	const initialValues: AppFormValues = app
		? {
				name: app.name,
				description: app.description ?? "",
				endpointUrl: app.endpoint?.url ?? "",
				endpointTimeout: app.endpoint?.timeout ?? 30,
				secret: "",
				status: app.status ?? (app.active ? "active" : "inactive"),
			}
		: APP_DEFAULTS;

	const handleSubmit = async (values: AppFormValues) => {
		const payload: AppResource = {
			resourceType: "App",
			name: values.name,
			description: values.description || undefined,
			status: values.status,
			endpoint: {
				url: values.endpointUrl,
				timeout: values.endpointTimeout,
			},
		};
		if (values.secret) payload.secret = values.secret;
		try {
			if (isEditing && app?.id) {
				await update.mutateAsync({ ...payload, id: app.id });
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
			title={isEditing ? "Edit Application" : "Create Application"}
			size="md"
		>
			<Form<AppFormValues>
				key={app?.id ?? "new"}
				onSubmit={handleSubmit}
				validate={makeAppValidator(isEditing)}
				initialValues={initialValues}
				render={({ handleSubmit: submit, submitting }) => (
					<form onSubmit={submit}>
						<Flex direction="column" gap="4">
							<Field<string> name="name">
								{({ input, meta }) => (
									<TextInput
										label="App Name"
										required
										placeholder="My Application"
										value={input.value}
										onChange={input.onChange}
										onBlur={input.onBlur}
										error={meta.touched && meta.error ? meta.error : undefined}
									/>
								)}
							</Field>

							<Field<string> name="description">
								{({ input }) => (
									<Textarea
										label="Description"
										placeholder="Brief description of your application"
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Field<string> name="endpointUrl">
								{({ input, meta }) => (
									<TextInput
										label="Endpoint URL"
										required
										placeholder="http://backend:3000/api"
										description="Backend URL for proxying requests"
										value={input.value}
										onChange={input.onChange}
										onBlur={input.onBlur}
										error={meta.touched && meta.error ? meta.error : undefined}
									/>
								)}
							</Field>

							<Field<number> name="endpointTimeout">
								{({ input }) => (
									<NumberInput
										label="Timeout (seconds)"
										description="Request timeout in seconds"
										value={input.value}
										onUpdate={(value) => input.onChange(value ?? 0)}
										min={1}
										max={300}
									/>
								)}
							</Field>

							<Field<string> name="secret">
								{({ input, meta }) => (
									<TextInput
										label={isEditing ? "Secret (leave empty to keep current)" : "Secret"}
										required={!isEditing}
										type="password"
										placeholder="Enter app secret"
										description="Used to authenticate requests from the backend"
										value={input.value}
										onChange={input.onChange}
										onBlur={input.onBlur}
										error={meta.touched && meta.error ? meta.error : undefined}
									/>
								)}
							</Field>

							<Field<string> name="status">
								{({ input }) => (
									<Select
										label="Status"
										data={[
											{ label: "Active", value: "active" },
											{ label: "Inactive", value: "inactive" },
											{ label: "Suspended", value: "suspended" },
										]}
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Flex justifyContent="flex-end" gap="2" className={classes.modalActions}>
								<Button variant="light" onClick={onClose} type="button">
									Cancel
								</Button>
								<Button
									type="submit"
									loading={submitting || create.isPending || update.isPending}
								>
									{isEditing ? "Update" : "Create"}
								</Button>
							</Flex>
						</Flex>
					</form>
				)}
			/>
		</Modal>
	);
}
