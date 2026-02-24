import { useParams, useNavigate } from "react-router-dom";
import {
	Stack,
	Title,
	Text,
	Paper,
	Group,
	Badge,
	Loader,
	Alert,
	Button,
	Code,
	Divider,
	Breadcrumbs,
	Anchor,
	Table,
	Tooltip,
	ThemeIcon,
	SimpleGrid,
	Box,
} from "@/shared/ui";
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
} from "@tabler/icons-react";
import { useApp } from "../lib/useApps";

const STATUS_COLORS: Record<string, string> = {
	active: "green",
	inactive: "gray",
	suspended: "fire",
};

const METHOD_COLORS: Record<string, string> = {
	GET: "primary",
	POST: "warm",
	PUT: "deep",
	DELETE: "fire",
	PATCH: "warm",
};

function MethodBadge({ method }: { method: string }) {
	return (
		<Badge size="xs" variant="light" color={METHOD_COLORS[method] ?? "gray"}>
			{method}
		</Badge>
	);
}

function InfoItem({ label, value, icon }: { label: string; value: React.ReactNode; icon?: React.ReactNode }) {
	return (
		<Box>
			<Group gap="xs" mb={4}>
				{icon}
				<Text size="xs" c="dimmed" tt="uppercase" fw={500}>
					{label}
				</Text>
			</Group>
			<Text size="sm" fw={500}>
				{value}
			</Text>
		</Box>
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

	const status = app?.status ?? (app?.active ? "active" : "inactive");
	const operations = app?.operations ?? [];
	const subscriptions = app?.subscriptions ?? [];

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }} className="page-enter">
			<Breadcrumbs>
				<Anchor onClick={() => navigate("/apps")}>Apps</Anchor>
				<Text>{app?.name ?? "Loading..."}</Text>
			</Breadcrumbs>

			<Group>
				<Button variant="subtle" leftSection={<IconArrowLeft size={16} />} onClick={() => navigate("/apps")}>
					Back
				</Button>
			</Group>

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
					{/* Header Card */}
					<Paper p="lg" style={{ backgroundColor: "var(--octo-surface-1)" }}>
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
									color={STATUS_COLORS[status] ?? "gray"}
								>
									{status}
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
					</Paper>

					{/* Details Card */}
					<Paper p="lg" style={{ backgroundColor: "var(--octo-surface-2)" }}>
						<Title order={5} mb="md">Configuration</Title>
						<SimpleGrid cols={{ base: 1, sm: 2, md: 4 }} spacing="lg">
							<InfoItem
								label="Endpoint URL"
								value={
									app.endpoint?.url ? (
										<Group gap="xs">
											<Code size="xs">{app.endpoint.url}</Code>
											<Tooltip label="Open endpoint">
												<Anchor href={app.endpoint.url} target="_blank" size="xs">
													<IconExternalLink size={14} />
												</Anchor>
											</Tooltip>
										</Group>
									) : (
										<Text size="sm" c="dimmed">Not configured</Text>
									)
								}
								icon={<IconWorld size={14} color="var(--mantine-color-dimmed)" />}
							/>
							<InfoItem
								label="Timeout"
								value={app.endpoint?.timeout ? `${app.endpoint.timeout}s` : "Default (30s)"}
								icon={<IconClock size={14} color="var(--mantine-color-dimmed)" />}
							/>
							<InfoItem
								label="API Version"
								value={app.apiVersion ?? 1}
								icon={<IconApi size={14} color="var(--mantine-color-dimmed)" />}
							/>
							<InfoItem
								label="Base Path"
								value={app.basePath || "None"}
								icon={<IconWorld size={14} color="var(--mantine-color-dimmed)" />}
							/>
						</SimpleGrid>
					</Paper>

					{/* Operations Section */}
					<Paper p="lg" style={{ backgroundColor: "var(--octo-surface-2)" }}>
						<Group justify="space-between" mb="md">
							<Group gap="sm">
								<IconApi size={20} color="var(--mantine-color-primary-6)" />
								<Title order={5}>Operations</Title>
								<Badge size="sm" variant="light" color="gray">
									{operations.length}
								</Badge>
							</Group>
						</Group>

						{operations.length === 0 ? (
							<Text size="sm" c="dimmed" ta="center" py="md">
								No operations defined. Add operations to the App manifest to expose custom API endpoints.
							</Text>
						) : (
							<Table striped highlightOnHover>
								<Table.Thead>
									<Table.Tr>
										<Table.Th>ID</Table.Th>
										<Table.Th>Method</Table.Th>
										<Table.Th>Path</Table.Th>
										<Table.Th>Access</Table.Th>
										<Table.Th>Policy</Table.Th>
									</Table.Tr>
								</Table.Thead>
								<Table.Tbody>
									{operations.map((op) => (
										<Table.Tr key={op.id}>
											<Table.Td>
												<Code size="xs">{op.id}</Code>
											</Table.Td>
											<Table.Td>
												<MethodBadge method={op.method} />
											</Table.Td>
											<Table.Td>
												<Code size="xs">
													/{Array.isArray(op.path) ? op.path.map(seg =>
														typeof seg === "string" ? seg : `:${seg.name}`
													).join("/") : op.path}
												</Code>
											</Table.Td>
											<Table.Td>
												<Tooltip label={op.public ? "Public (no auth required)" : "Protected (requires auth)"}>
													{op.public ? (
														<Badge size="xs" color="primary" variant="light" leftSection={<IconLockOpen size={10} />}>
															Public
														</Badge>
													) : (
														<Badge size="xs" color="deep" variant="light" leftSection={<IconLock size={10} />}>
															Protected
														</Badge>
													)}
												</Tooltip>
											</Table.Td>
											<Table.Td>
												{op.policy ? (
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
													<Text size="xs" c="dimmed">—</Text>
												)}
											</Table.Td>
										</Table.Tr>
									))}
								</Table.Tbody>
							</Table>
						)}
					</Paper>

					{/* Subscriptions Section */}
					<Paper p="lg" style={{ backgroundColor: "var(--octo-surface-2)" }}>
						<Group justify="space-between" mb="md">
							<Group gap="sm">
								<IconWebhook size={20} color="var(--mantine-color-warm-6)" />
								<Title order={5}>Subscriptions</Title>
								<Badge size="sm" variant="light" color="gray">
									{subscriptions.length}
								</Badge>
							</Group>
						</Group>

						{subscriptions.length === 0 ? (
							<Text size="sm" c="dimmed" ta="center" py="md">
								No subscriptions defined. Add subscriptions to receive webhooks on FHIR resource events.
							</Text>
						) : (
							<Table striped highlightOnHover>
								<Table.Thead>
									<Table.Tr>
										<Table.Th>ID</Table.Th>
										<Table.Th>Resource</Table.Th>
										<Table.Th>Event</Table.Th>
										<Table.Th>Filter</Table.Th>
										<Table.Th>Channel</Table.Th>
									</Table.Tr>
								</Table.Thead>
								<Table.Tbody>
									{subscriptions.map((sub) => (
										<Table.Tr key={sub.id}>
											<Table.Td>
												<Code size="xs">{sub.id}</Code>
											</Table.Td>
											<Table.Td>
												<Badge size="xs" variant="light" color="primary">
													{sub.trigger.resourceType}
												</Badge>
											</Table.Td>
											<Table.Td>
												<Badge
													size="xs"
													variant="outline"
													color={
														sub.trigger.event === "create" ? "green" :
														sub.trigger.event === "update" ? "blue" :
														sub.trigger.event === "delete" ? "fire" : "gray"
													}
												>
													{sub.trigger.event}
												</Badge>
											</Table.Td>
											<Table.Td>
												{sub.trigger.fhirpath ? (
													<Tooltip label={sub.trigger.fhirpath}>
														<Code size="xs" style={{ maxWidth: 150 }} lineClamp={1}>
															{sub.trigger.fhirpath}
														</Code>
													</Tooltip>
												) : (
													<Text size="xs" c="dimmed">All</Text>
												)}
											</Table.Td>
											<Table.Td>
												{sub.channel ? (
													<Group gap={4}>
														<Badge size="xs" variant="light" color="deep">
															{sub.channel.type}
														</Badge>
														<Tooltip label={sub.channel.endpoint}>
															<Code size="xs" style={{ maxWidth: 120 }} lineClamp={1}>
																{sub.channel.endpoint}
															</Code>
														</Tooltip>
													</Group>
												) : sub.notification ? (
													<Group gap={4}>
														<IconBell size={12} color="var(--mantine-color-warm-6)" />
														<Badge size="xs" variant="light" color="warm">
															notification
														</Badge>
													</Group>
												) : (
													<Text size="xs" c="dimmed">—</Text>
												)}
											</Table.Td>
										</Table.Tr>
									))}
								</Table.Tbody>
							</Table>
						)}
					</Paper>
				</>
			)}
		</Stack>
	);
}
