import { Field, Form, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import {
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
	Skeleton,
	EmptyState,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
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
	const [deletingApp, setDeletingApp] = useState<AppResource | null>(null);

	const { data, isLoading, isError, error, refetch } = useApps({ search: debouncedSearch });
	const deleteApp = useDeleteApp();

	const handleView = (id: string) => {
		navigate(`/apps/${id}`);
	};

	const handleEdit = (app: AppResource) => {
		setEditingApp(app);
		open();
	};

	const confirmDelete = () => {
		if (deletingApp?.id) {
			deleteApp.mutate(deletingApp.id, {
				onSettled: () => setDeletingApp(null),
			});
		}
	};

	const handleClose = () => {
		setEditingApp(null);
		close();
	};

	const apps = getBundleResources<AppResource>(data);
	const hasSearch = debouncedSearch.trim().length > 0;

	return (
		<WorkspacePageLayout
			title="API Gateway Apps"
			description="Group custom operations under base paths and common configuration"
			actions={
				<Button leftSection={<IconPlus width={16} height={16} aria-hidden="true" />} onClick={open}>
					Create App
				</Button>
			}
			toolbar={
				<div className={classes.toolbar}>
					<TextInput
						aria-label="Search applications by name"
						placeholder="Search by name..."
						leftSection={<IconSearch width={16} height={16} aria-hidden="true" />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						className={classes.searchInput}
					/>
				</div>
			}
			maxWidth={1280}
		>

			{isLoading ? (
				<div className={classes.tablePanel}>
					<div className={classes.skeletonList}>
						{Array.from({ length: 5 }).map((_, i) => (
							// biome-ignore lint/suspicious/noArrayIndexKey: static skeleton placeholders
							<Skeleton key={i} className={classes.skeletonRow} />
						))}
					</div>
				</div>
			) : isError ? (
				<div className={classes.tablePanel}>
					<EmptyState
						image={<IconRocket width={48} height={48} aria-hidden="true" />}
						title="Couldn't load applications"
						description={error instanceof Error ? error.message : "The application list failed to load."}
						actions={[
							<Button key="retry" view="action" onClick={() => refetch()}>
								Retry
							</Button>,
						]}
					/>
				</div>
			) : apps.length === 0 ? (
				<div className={classes.tablePanel}>
					<EmptyState
						image={<IconRocket width={48} height={48} aria-hidden="true" />}
						title={hasSearch ? "No matching applications" : "No applications yet"}
						description={
							hasSearch
								? "No applications match your search. Try a different name or clear the filter."
								: "Create an API Gateway App to group custom operations under a base path."
						}
						actions={
							hasSearch
								? [
										<Button key="clear" view="outlined" onClick={() => setSearch("")}>
											Clear filters
										</Button>,
									]
								: [
										<Button key="create" view="action" onClick={open}>
											Create App
										</Button>,
									]
						}
					/>
				</div>
			) : (
				<div className={classes.tablePanel}>
					<DataPreview
						columns={[
							{ id: "application", label: "Application" },
							{ id: "endpoint", label: "Endpoint", width: 240 },
							{ id: "operations", label: "Operations", width: 140 },
							{ id: "status", label: "Status", width: 110 },
							{ id: "actions", label: "", width: 48 },
						]}
						rows={apps.map((app) => {
							const statusView = getAppStatusView(app);
							const endpoint = getAppEndpointDisplay(app);

							return {
								application: (
									<div className={classes.appCell}>
										<IconRocket
											width={16}
											height={16}
											color="var(--octo-brand-primary-active)"
											aria-hidden="true"
										/>
										<div className={classes.appSummary}>
											<Anchor onClick={() => app.id && handleView(app.id)}>{app.name}</Anchor>
											<Text variant="caption-2" color="secondary" ellipsis className={classes.truncateText}>
												{app.description || "No description"}
											</Text>
										</div>
									</div>
								),
								endpoint:
									app.endpoint?.url || app.basePath ? (
										<Code className={classes.endpointCode}>{endpoint}</Code>
									) : (
										<Text variant="caption-2" color="secondary">
											{endpoint}
										</Text>
									),
								operations: (
									<div className={classes.operationBadges}>
										{app.operations && app.operations.length > 0 && (
											<Badge
												size="xs"
												color="primary"
												leftSection={<IconApi width={10} height={10} aria-hidden="true" />}
											>
												{app.operations.length}
											</Badge>
										)}
										{app.subscriptions && app.subscriptions.length > 0 && (
											<Badge
												size="xs"
												color="warm"
												leftSection={<IconWebhook width={10} height={10} aria-hidden="true" />}
											>
												{app.subscriptions.length}
											</Badge>
										)}
										{(!app.operations || app.operations.length === 0) &&
											(!app.subscriptions || app.subscriptions.length === 0) && (
												<Text variant="caption-2" color="secondary">
													-
												</Text>
											)}
									</div>
								),
								status: (
									<Badge color={statusView.color} size="sm">
										{statusView.status}
									</Badge>
								),
								actions: (
									<Menu placement="bottom-end">
										<Menu.Target>
											<ActionIcon
												view="flat"
												size="s"
												aria-label={`Actions for ${app.name}`}
												aria-haspopup="menu"
											>
												<IconDotsVertical width={16} height={16} aria-hidden="true" />
											</ActionIcon>
										</Menu.Target>
										<Menu.Dropdown>
											<Menu.Item
												leftSection={<IconEye width={14} height={14} aria-hidden="true" />}
												onClick={() => app.id && handleView(app.id)}
											>
												View Details
											</Menu.Item>
											<Menu.Item
												leftSection={<IconEdit width={14} height={14} aria-hidden="true" />}
												onClick={() => handleEdit(app)}
											>
												Edit JSON
											</Menu.Item>
											{app.endpoint?.url && (
												<Menu.Item
													leftSection={<IconExternalLink width={14} height={14} aria-hidden="true" />}
													component="a"
													href={app.endpoint.url}
													target="_blank"
												>
													Open Endpoint
												</Menu.Item>
											)}
											<Menu.Divider />
											<Menu.Item
												leftSection={<IconTrash width={14} height={14} aria-hidden="true" />}
												color="danger"
												onClick={() => setDeletingApp(app)}
											>
												Delete
											</Menu.Item>
										</Menu.Dropdown>
									</Menu>
								),
							};
						})}
						getRowKey={(_row, index) => apps[index]?.id ?? `${index}`}
					/>
				</div>
			)}

			<AppModal opened={opened} onClose={handleClose} app={editingApp} />

			<Modal
				open={deletingApp != null}
				onClose={() => setDeletingApp(null)}
				title="Delete application"
				size="s"
				footer={
					<div className={classes.modalActions}>
						<Button view="flat" onClick={() => setDeletingApp(null)} type="button">
							Cancel
						</Button>
						<Button
							view="flat-danger"
							loading={deleteApp.isPending}
							onClick={confirmDelete}
							type="button"
						>
							Delete
						</Button>
					</div>
				}
			>
				<Text variant="body-2">
					Are you sure you want to delete{" "}
					<strong>{deletingApp?.name ?? "this application"}</strong>? This action cannot be undone.
				</Text>
			</Modal>
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
			open={opened}
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
						<div className={classes.modalForm}>
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

							<div className={classes.modalActions}>
								<Button view="outlined" onClick={onClose} type="button">
									Cancel
								</Button>
								<Button
									type="submit"
									loading={submitting || create.isPending || update.isPending}
								>
									{isEditing ? "Update" : "Create"}
								</Button>
							</div>
						</div>
					</form>
				)}
			/>
		</Modal>
	);
}
