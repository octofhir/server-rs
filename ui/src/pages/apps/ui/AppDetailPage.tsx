import { useParams, useNavigate } from "react-router-dom";
import {
	Title,
	Text,
	Group,
	Badge,
	Loader,
	Alert,
	Button,
	Code,
	Breadcrumbs,
	Anchor,
	Tooltip,
	ThemeIcon,
	DataPreview,
	KeyValueList,
	SectionPanel,
} from "@/shared/ui";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import {
	IconAlertCircle,
	IconArrowLeft,
	IconRocket,
	IconWorld,
	IconClock,
	IconLock,
	IconLockOpen,
	IconApi,
	IconWebhook,
	IconBell,
	IconEdit,
	IconExternalLink,
} from "@octofhir/ui-kit";
import {
	formatAppOperationPath,
	getAppEndpointDisplay,
	getAppMethodView,
	getAppOperationAccessView,
	getAppStatusView,
	getSubscriptionEventView,
} from "@/entities/api-app";
import { useApp } from "../lib/useApps";
import classes from "./AppDetailPage.module.css";

function MethodBadge({ method }: { method: string }) {
	const methodView = getAppMethodView(method);

	return (
		<Badge size="xs" variant="light" color={methodView.color}>
			{methodView.method}
		</Badge>
	);
}

export function AppDetailPage() {
	const { id } = useParams<{ id: string }>();
	const navigate = useNavigate();
	const { data: app, isLoading, error } = useApp(id ?? null);

	if (!id) {
		return (
			<Alert icon={<IconAlertCircle size={16} />} color="fire" variant="light">
				App ID is required
			</Alert>
		);
	}

	const statusView = app ? getAppStatusView(app) : null;
	const operations = app?.operations ?? [];
	const subscriptions = app?.subscriptions ?? [];

	return (
		<WorkspacePageLayout
			title={app?.name ?? "App details"}
			description="FHIR App resource backing custom API surfaces"
			className="page-enter"
			kicker={
				<Breadcrumbs>
				<Anchor onClick={() => navigate("/apps")}>Apps</Anchor>
				<Text>{app?.name ?? "Loading..."}</Text>
				</Breadcrumbs>
			}
			actions={
				<Button variant="subtle" leftSection={<IconArrowLeft size={16} />} onClick={() => navigate("/apps")}>
					Back
				</Button>
			}
		>

			{isLoading && (
				<Group justify="center" py="xl">
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Loading app...
					</Text>
				</Group>
			)}

			{error && (
				<Alert icon={<IconAlertCircle size={16} />} color="fire" variant="light">
					{error instanceof Error ? error.message : "Failed to load app"}
				</Alert>
			)}

			{app && (
				<>
					<SectionPanel
						title="App summary"
						description="FHIR App resource backing custom API surfaces"
						view="filled"
						padding="l"
					>
						<Group justify="space-between" align="flex-start">
							<Group gap="md">
								<ThemeIcon size="xl" variant="light" color="primary" radius="md">
									<IconRocket size={24} />
								</ThemeIcon>
								<div>
									<Title order={3}>{app.name}</Title>
									{app.description && (
										<Text size="sm" c="dimmed" maw={500}>
											{app.description}
										</Text>
									)}
									<Code size="xs" mt="xs">{app.id}</Code>
								</div>
							</Group>
							<Group gap="xs">
								<Badge
									size="lg"
									variant="light"
									color={statusView?.color ?? "gray"}
								>
									{statusView?.status}
								</Badge>
								<Button
									variant="light"
									size="xs"
									leftSection={<IconEdit size={14} />}
									onClick={() => navigate(`/resources/App/${id}`)}
								>
									Edit
								</Button>
							</Group>
						</Group>
					</SectionPanel>

					<SectionPanel
						title="Configuration"
						description="Endpoint, timeout, API version, and legacy base path"
						view="tinted"
						padding="l"
					>
						<KeyValueList
							items={[
								{
									id: "endpoint",
									label: (
										<Group gap="xs">
											<IconWorld size={14} color="var(--g-color-text-secondary)" />
											Endpoint URL
										</Group>
									),
									value: app.endpoint?.url ? (
										<Group gap="xs">
											<Code size="xs">{app.endpoint.url}</Code>
											<Tooltip label="Open endpoint">
												<Anchor href={app.endpoint.url} target="_blank" size="xs">
													<IconExternalLink size={14} />
												</Anchor>
											</Tooltip>
										</Group>
									) : (
										<Text size="sm" c="dimmed">
											{getAppEndpointDisplay(app)}
										</Text>
									),
								},
								{
									id: "timeout",
									label: (
										<Group gap="xs">
											<IconClock size={14} color="var(--g-color-text-secondary)" />
											Timeout
										</Group>
									),
									value: app.endpoint?.timeout ? `${app.endpoint.timeout}s` : "Default (30s)",
								},
								{
									id: "api-version",
									label: (
										<Group gap="xs">
											<IconApi size={14} color="var(--g-color-text-secondary)" />
											API Version
										</Group>
									),
									value: app.apiVersion ?? 1,
								},
								{
									id: "base-path",
									label: (
										<Group gap="xs">
											<IconWorld size={14} color="var(--g-color-text-secondary)" />
											Base Path
										</Group>
									),
									value: app.basePath || "None",
								},
							]}
						/>
					</SectionPanel>

					<SectionPanel
						title={
							<Group gap="sm">
								<IconApi size={20} color="var(--octo-brand-primary-active)" />
								Operations
								<Badge size="sm" variant="light" color="gray">
									{operations.length}
								</Badge>
							</Group>
						}
						description="Inline API operation contracts exposed by this App"
						view="tinted"
						padding="l"
					>
						<DataPreview
							columns={[
								{ id: "id", label: "ID", width: 180 },
								{ id: "method", label: "Method", width: 100 },
								{ id: "path", label: "Path" },
								{ id: "access", label: "Access", width: 130 },
								{ id: "policy", label: "Policy", width: 180 },
							]}
							rows={operations.map((op) => {
								const accessView = getAppOperationAccessView(op.public);

								return {
									id: <Code size="xs">{op.id}</Code>,
									method: <MethodBadge method={op.method} />,
									path: <Code size="xs">{formatAppOperationPath(op.path)}</Code>,
									access: (
										<Tooltip label={accessView.description}>
											<Badge
												size="xs"
												color={accessView.color}
												variant="light"
												leftSection={op.public ? <IconLockOpen size={10} /> : <IconLock size={10} />}
											>
												{accessView.label}
											</Badge>
										</Tooltip>
									),
									policy: op.policy ? (
										<Group gap={4}>
											{op.policy.roles && (
												<Tooltip label={`Roles: ${op.policy.roles.join(", ")}`}>
													<Badge size="xs" variant="outline" color="violet">
														{op.policy.roles.length} role(s)
													</Badge>
												</Tooltip>
											)}
											{op.policy.compartment && (
												<Badge size="xs" variant="outline" color="blue">
													{op.policy.compartment}
												</Badge>
											)}
										</Group>
									) : (
										<Text size="xs" c="dimmed">-</Text>
									),
								};
							})}
							emptyText="No operations defined. Add operations to the App manifest to expose custom API endpoints."
							getRowKey={(_row, index) => operations[index]?.id ?? `${index}`}
						/>
					</SectionPanel>

					<SectionPanel
						title={
							<Group gap="sm">
								<IconWebhook size={20} color="var(--g-color-base-warning-medium-hover)" />
								Subscriptions
								<Badge size="sm" variant="light" color="gray">
									{subscriptions.length}
								</Badge>
							</Group>
						}
						description="FHIR event subscriptions and webhook or notification channels"
						view="tinted"
						padding="l"
					>
						<DataPreview
							columns={[
								{ id: "id", label: "ID", width: 180 },
								{ id: "resource", label: "Resource", width: 130 },
								{ id: "event", label: "Event", width: 100 },
								{ id: "filter", label: "Filter" },
								{ id: "channel", label: "Channel", width: 220 },
							]}
							rows={subscriptions.map((sub) => {
								const eventView = getSubscriptionEventView(sub.trigger.event);

								return {
									id: <Code size="xs">{sub.id}</Code>,
									resource: (
										<Badge size="xs" variant="light" color="primary">
											{sub.trigger.resourceType}
										</Badge>
									),
									event: (
										<Badge size="xs" variant="outline" color={eventView.color}>
											{eventView.event}
										</Badge>
									),
									filter: sub.trigger.fhirpath ? (
										<Tooltip label={sub.trigger.fhirpath}>
											<Code size="xs" className={classes.filterCode}>
												{sub.trigger.fhirpath}
											</Code>
										</Tooltip>
									) : (
										<Text size="xs" c="dimmed">All</Text>
									),
									channel: sub.channel ? (
										<Group gap={4}>
											<Badge size="xs" variant="light" color="deep">
												{sub.channel.type}
											</Badge>
											<Tooltip label={sub.channel.endpoint}>
												<Code size="xs" className={classes.endpointCode}>
													{sub.channel.endpoint}
												</Code>
											</Tooltip>
										</Group>
									) : sub.notification ? (
										<Group gap={4}>
											<IconBell size={12} color="var(--g-color-base-warning-medium-hover)" />
											<Badge size="xs" variant="light" color="warm">
												notification
											</Badge>
										</Group>
									) : (
										<Text size="xs" c="dimmed">-</Text>
									),
								};
							})}
							emptyText="No subscriptions defined. Add subscriptions to receive webhooks on FHIR resource events."
							getRowKey={(_row, index) => subscriptions[index]?.id ?? `${index}`}
						/>
					</SectionPanel>
				</>
			)}
		</WorkspacePageLayout>
	);
}
