import { useParams, useNavigate } from "react-router-dom";
import {
	Text,
	Badge,
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
	Skeleton,
	EmptyState,
} from "@octofhir/ui-kit";
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
		<Badge size="xs" color={methodView.color}>
			{methodView.method}
		</Badge>
	);
}

export function AppDetailPage() {
	const { id } = useParams<{ id: string }>();
	const navigate = useNavigate();
	const { data: app, isLoading, isError, error, refetch } = useApp(id ?? null);

	if (!id) {
		return (
			<Alert theme="danger" title="App ID is required" message="No application identifier was provided." />
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
				<Button
					variant="subtle"
					leftSection={<IconArrowLeft width={16} height={16} aria-hidden="true" />}
					onClick={() => navigate("/apps")}
				>
					Back
				</Button>
			}
		>

			{isLoading && (
				<div className={classes.loadingState}>
					<Skeleton className={classes.skeletonSummary} />
					<Skeleton className={classes.skeletonPanel} />
					<Skeleton className={classes.skeletonPanel} />
				</div>
			)}

			{isError && (
				<EmptyState
					image={<IconAlertCircle width={48} height={48} aria-hidden="true" />}
					title="Couldn't load application"
					description={error instanceof Error ? error.message : "Failed to load app"}
					actions={[
						<Button key="retry" variant="filled" onClick={() => refetch()}>
							Retry
						</Button>,
					]}
				/>
			)}

			{app && (
				<>
					<SectionPanel
						title="App summary"
						description="FHIR App resource backing custom API surfaces"
						view="filled"
						padding="l"
					>
						<div className={classes.summaryHeader}>
							<div className={classes.summaryMain}>
								<ThemeIcon size="xl" view="light" color="primary" radius="md">
									<IconRocket width={24} height={24} aria-hidden="true" />
								</ThemeIcon>
								<div className={classes.summaryText}>
									<Text variant="subheader-2" as="h3">
										{app.name}
									</Text>
									{app.description && (
										<Text variant="body-2" color="secondary" className={classes.summaryDescription}>
											{app.description}
										</Text>
									)}
									<Code className={classes.summaryId}>{app.id}</Code>
								</div>
							</div>
							<div className={classes.summaryActions}>
								<Badge size="lg" color={statusView?.color ?? "gray"}>
									{statusView?.status}
								</Badge>
								<Button
									variant="outline"
									size="sm"
									leftSection={<IconEdit width={14} height={14} aria-hidden="true" />}
									onClick={() => navigate(`/resources/App/${id}`)}
								>
									Edit
								</Button>
							</div>
						</div>
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
										<div className={classes.labelInline}>
											<IconWorld width={14} height={14} color="var(--g-color-text-secondary)" aria-hidden="true" />
											Endpoint URL
										</div>
									),
									value: app.endpoint?.url ? (
										<div className={classes.valueInline}>
											<Code>{app.endpoint.url}</Code>
											<Tooltip label="Open endpoint">
												<Anchor href={app.endpoint.url} target="_blank" aria-label="Open endpoint in new tab">
													<IconExternalLink width={14} height={14} aria-hidden="true" />
												</Anchor>
											</Tooltip>
										</div>
									) : (
										<Text variant="body-2" color="secondary">
											{getAppEndpointDisplay(app)}
										</Text>
									),
								},
								{
									id: "timeout",
									label: (
										<div className={classes.labelInline}>
											<IconClock width={14} height={14} color="var(--g-color-text-secondary)" aria-hidden="true" />
											Timeout
										</div>
									),
									value: app.endpoint?.timeout ? `${app.endpoint.timeout}s` : "Default (30s)",
								},
								{
									id: "api-version",
									label: (
										<div className={classes.labelInline}>
											<IconApi width={14} height={14} color="var(--g-color-text-secondary)" aria-hidden="true" />
											API Version
										</div>
									),
									value: app.apiVersion ?? 1,
								},
								{
									id: "base-path",
									label: (
										<div className={classes.labelInline}>
											<IconWorld width={14} height={14} color="var(--g-color-text-secondary)" aria-hidden="true" />
											Base Path
										</div>
									),
									value: app.basePath || "None",
								},
							]}
						/>
					</SectionPanel>

					<SectionPanel
						title={
							<div className={classes.sectionTitle}>
								<IconApi width={20} height={20} color="var(--octo-brand-primary-active)" aria-hidden="true" />
								Operations
								<Badge size="sm" color="gray">
									{operations.length}
								</Badge>
							</div>
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
									id: <Code>{op.id}</Code>,
									method: <MethodBadge method={op.method} />,
									path: <Code>{formatAppOperationPath(op.path)}</Code>,
									access: (
										<Tooltip label={accessView.description}>
											<Badge
												size="xs"
												color={accessView.color}
												leftSection={
													op.public ? (
														<IconLockOpen width={10} height={10} aria-hidden="true" />
													) : (
														<IconLock width={10} height={10} aria-hidden="true" />
													)
												}
											>
												{accessView.label}
											</Badge>
										</Tooltip>
									),
									policy: op.policy ? (
										<div className={classes.policyBadges}>
											{op.policy.roles && (
												<Tooltip label={`Roles: ${op.policy.roles.join(", ")}`}>
													<Badge size="xs" color="deep">
														{op.policy.roles.length} role(s)
													</Badge>
												</Tooltip>
											)}
											{op.policy.compartment && (
												<Badge size="xs" color="primary">
													{op.policy.compartment}
												</Badge>
											)}
										</div>
									) : (
										<Text variant="caption-2" color="secondary">
											-
										</Text>
									),
								};
							})}
							emptyText="No operations defined. Add operations to the App manifest to expose custom API endpoints."
							getRowKey={(_row, index) => operations[index]?.id ?? `${index}`}
						/>
					</SectionPanel>

					<SectionPanel
						title={
							<div className={classes.sectionTitle}>
								<IconWebhook
									width={20}
									height={20}
									color="var(--g-color-base-warning-medium-hover)"
									aria-hidden="true"
								/>
								Subscriptions
								<Badge size="sm" color="gray">
									{subscriptions.length}
								</Badge>
							</div>
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
									id: <Code>{sub.id}</Code>,
									resource: (
										<Badge size="xs" color="primary">
											{sub.trigger.resourceType}
										</Badge>
									),
									event: (
										<Badge size="xs" color={eventView.color}>
											{eventView.event}
										</Badge>
									),
									filter: sub.trigger.fhirpath ? (
										<Tooltip label={sub.trigger.fhirpath}>
											<Code className={classes.filterCode}>{sub.trigger.fhirpath}</Code>
										</Tooltip>
									) : (
										<Text variant="caption-2" color="secondary">
											All
										</Text>
									),
									channel: sub.channel ? (
										<div className={classes.channelBadges}>
											<Badge size="xs" color="deep">
												{sub.channel.type}
											</Badge>
											<Tooltip label={sub.channel.endpoint}>
												<Code className={classes.endpointCode}>{sub.channel.endpoint}</Code>
											</Tooltip>
										</div>
									) : sub.notification ? (
										<div className={classes.channelBadges}>
											<IconBell
												width={12}
												height={12}
												color="var(--g-color-base-warning-medium-hover)"
												aria-hidden="true"
											/>
											<Badge size="xs" color="warm">
												notification
											</Badge>
										</div>
									) : (
										<Text variant="caption-2" color="secondary">
											-
										</Text>
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
