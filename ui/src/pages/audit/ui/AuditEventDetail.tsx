import { memo } from "react";
import {
	Stack,
	Group,
	Text,
	Badge,
	Divider,
	Tabs,
	Code,
	ThemeIcon,
	ScrollArea,
	CopyButton,
	ActionIcon,
	Tooltip,
	Paper,
} from "@mantine/core";
import {
	IconUser,
	IconServer,
	IconAppWindow,
	IconClock,
	IconNetwork,
	IconDeviceDesktop,
	IconCheck,
	IconX,
	IconAlertTriangle,
	IconCopy,
	IconPlus,
	IconMinus,
	IconArrowRight,
	IconArrowsExchange,
} from "@tabler/icons-react";
import type { AuditEvent, AuditAction, AuditOutcome } from "@/shared/api/types";
import classes from "./AuditEventDetail.module.css";

interface AuditEventDetailProps {
	event: AuditEvent;
}

function getActionLabel(action: AuditAction): string {
	const labels: Record<AuditAction, string> = {
		"user.login": "User Login",
		"user.logout": "User Logout",
		"user.login_failed": "Login Failed",
		"resource.create": "Resource Created",
		"resource.read": "Resource Read",
		"resource.update": "Resource Updated",
		"resource.delete": "Resource Deleted",
		"resource.search": "Resource Search",
		"policy.evaluate": "Policy Evaluated",
		"client.auth": "Client Authentication",
		"client.create": "Client Created",
		"client.update": "Client Updated",
		"client.delete": "Client Deleted",
		"config.change": "Configuration Changed",
		"system.startup": "System Started",
		"system.shutdown": "System Stopped",
	};
	return labels[action] || action;
}

function getOutcomeColor(outcome: AuditOutcome): string {
	switch (outcome) {
		case "success":
			return "green";
		case "failure":
			return "red";
		case "partial":
			return "yellow";
	}
}

function getOutcomeIcon(outcome: AuditOutcome) {
	switch (outcome) {
		case "success":
			return IconCheck;
		case "failure":
			return IconX;
		case "partial":
			return IconAlertTriangle;
	}
}

function getActorIcon(type: "user" | "client" | "system") {
	switch (type) {
		case "user":
			return IconUser;
		case "client":
			return IconAppWindow;
		case "system":
			return IconServer;
	}
}

function formatDateTime(timestamp: string): string {
	const d = new Date(timestamp);
	return d.toLocaleString();
}

function DetailRow({
	label,
	value,
	icon: Icon,
	copyable = false,
}: {
	label: string;
	value?: string | null;
	icon?: typeof IconUser;
	copyable?: boolean;
}) {
	if (!value) return null;

	return (
		<Group gap="sm" wrap="nowrap" className={classes.detailRow}>
			{Icon && (
				<ThemeIcon size="sm" variant="light" color="gray">
					<Icon size={12} />
				</ThemeIcon>
			)}
			<Text size="sm" c="dimmed" className={classes.detailLabel}>
				{label}
			</Text>
			<Group gap="xs" wrap="nowrap" style={{ flex: 1 }}>
				<Text size="sm" className={classes.detailValue}>
					{value}
				</Text>
				{copyable && (
					<CopyButton value={value}>
						{({ copied, copy }) => (
							<Tooltip label={copied ? "Copied" : "Copy"}>
								<ActionIcon size="xs" variant="subtle" onClick={copy}>
									{copied ? <IconCheck size={12} /> : <IconCopy size={12} />}
								</ActionIcon>
							</Tooltip>
						)}
					</CopyButton>
				)}
			</Group>
		</Group>
	);
}

function DiffViewer({
	changes,
}: {
	changes: AuditEvent["changes"];
}) {
	if (!changes) return null;

	const { diff, before, after } = changes;

	// If we have a diff array, show it
	if (diff && diff.length > 0) {
		return (
			<Stack gap="xs">
				{diff.map((change) => (
					<Paper key={`${change.op}-${change.path}`} className={classes.diffItem} p="xs" withBorder>
						<Group gap="xs" wrap="nowrap" mb="xs">
							<Badge
								size="xs"
								variant="light"
								color={
									change.op === "add"
										? "green"
										: change.op === "remove"
											? "red"
											: "yellow"
								}
								leftSection={
									change.op === "add" ? (
										<IconPlus size={10} />
									) : change.op === "remove" ? (
										<IconMinus size={10} />
									) : (
										<IconArrowsExchange size={10} />
									)
								}
							>
								{change.op}
							</Badge>
							<Code size="xs">{change.path}</Code>
						</Group>
						<Group gap="sm" wrap="nowrap" align="flex-start">
							{change.op !== "add" && change.oldValue !== undefined && (
								<Paper className={classes.diffOld} p="xs" style={{ flex: 1 }}>
									<Text size="xs" c="dimmed" mb={4}>
										Before
									</Text>
									<Code block size="xs">
										{JSON.stringify(change.oldValue, null, 2)}
									</Code>
								</Paper>
							)}
							{change.op !== "remove" && change.newValue !== undefined && (
								<Paper className={classes.diffNew} p="xs" style={{ flex: 1 }}>
									<Text size="xs" c="dimmed" mb={4}>
										After
									</Text>
									<Code block size="xs">
										{JSON.stringify(change.newValue, null, 2)}
									</Code>
								</Paper>
							)}
						</Group>
					</Paper>
				))}
			</Stack>
		);
	}

	// Fall back to before/after comparison
	if (before || after) {
		return (
			<Group gap="md" align="flex-start" wrap="nowrap">
				{before && (
					<Paper className={classes.diffOld} p="sm" style={{ flex: 1 }}>
						<Text size="sm" fw={500} mb="sm">
							Before
						</Text>
						<ScrollArea.Autosize mah={300}>
							<Code block size="xs">
								{JSON.stringify(before, null, 2)}
							</Code>
						</ScrollArea.Autosize>
					</Paper>
				)}
				{before && after && (
					<ThemeIcon size="lg" variant="light" color="gray" className={classes.diffArrow}>
						<IconArrowRight size={16} />
					</ThemeIcon>
				)}
				{after && (
					<Paper className={classes.diffNew} p="sm" style={{ flex: 1 }}>
						<Text size="sm" fw={500} mb="sm">
							After
						</Text>
						<ScrollArea.Autosize mah={300}>
							<Code block size="xs">
								{JSON.stringify(after, null, 2)}
							</Code>
						</ScrollArea.Autosize>
					</Paper>
				)}
			</Group>
		);
	}

	return <Text c="dimmed" size="sm">No changes recorded</Text>;
}

function AuditEventDetailComponent({ event }: AuditEventDetailProps) {
	const OutcomeIcon = getOutcomeIcon(event.outcome);
	const ActorIcon = getActorIcon(event.actor.type);
	const hasChanges = event.changes && (event.changes.diff || event.changes.before || event.changes.after);
	const hasContext = event.context && Object.keys(event.context).length > 0;

	return (
		<div className={classes.container}>
			<ScrollArea className={classes.scrollArea}>
				<Stack gap="md" p="md">
					{/* Header */}
					<div className={classes.header}>
						<Group justify="space-between" align="flex-start">
							<Stack gap="xs">
								<Text size="lg" fw={600}>
									{getActionLabel(event.action)}
								</Text>
								<Group gap="xs">
									<Badge
										size="lg"
										variant="light"
										color={getOutcomeColor(event.outcome)}
										leftSection={<OutcomeIcon size={12} />}
									>
										{event.outcome.charAt(0).toUpperCase() + event.outcome.slice(1)}
									</Badge>
									{event.outcomeDescription && (
										<Text size="sm" c="dimmed">
											{event.outcomeDescription}
										</Text>
									)}
								</Group>
							</Stack>
							<CopyButton value={event.id}>
								{({ copied, copy }) => (
									<Tooltip label={copied ? "Copied!" : "Copy Event ID"}>
										<Badge
											size="sm"
											variant="light"
											color="gray"
											style={{ cursor: "pointer" }}
											onClick={copy}
										>
											{event.id.slice(0, 8)}...
										</Badge>
									</Tooltip>
								)}
							</CopyButton>
						</Group>
					</div>

					<Divider />

					{/* Tabs */}
					<Tabs defaultValue="details" variant="outline">
						<Tabs.List>
							<Tabs.Tab value="details">Details</Tabs.Tab>
							{hasChanges && <Tabs.Tab value="changes">Changes</Tabs.Tab>}
							{hasContext && <Tabs.Tab value="context">Context</Tabs.Tab>}
							<Tabs.Tab value="raw">Raw JSON</Tabs.Tab>
						</Tabs.List>

						<Tabs.Panel value="details" pt="md">
							<Stack gap="lg">
								{/* Timestamp */}
								<div>
									<Text size="xs" c="dimmed" tt="uppercase" mb="xs">
										Timestamp
									</Text>
									<DetailRow
										label="When"
										value={formatDateTime(event.timestamp)}
										icon={IconClock}
									/>
								</div>

								{/* Actor */}
								<div>
									<Text size="xs" c="dimmed" tt="uppercase" mb="xs">
										Actor
									</Text>
									<Stack gap="xs">
										<DetailRow
											label="Type"
											value={event.actor.type.charAt(0).toUpperCase() + event.actor.type.slice(1)}
											icon={ActorIcon}
										/>
										{event.actor.name && (
											<DetailRow label="Name" value={event.actor.name} />
										)}
										{event.actor.id && (
											<DetailRow label="ID" value={event.actor.id} copyable />
										)}
										{event.actor.reference && (
											<DetailRow label="Reference" value={event.actor.reference} copyable />
										)}
									</Stack>
								</div>

								{/* Source */}
								<div>
									<Text size="xs" c="dimmed" tt="uppercase" mb="xs">
										Source
									</Text>
									<Stack gap="xs">
										<DetailRow
											label="IP Address"
											value={event.source.ipAddress}
											icon={IconNetwork}
											copyable
										/>
										<DetailRow
											label="User Agent"
											value={event.source.userAgent}
											icon={IconDeviceDesktop}
										/>
										{event.source.site && (
											<DetailRow label="Site" value={event.source.site} />
										)}
									</Stack>
								</div>

								{/* Target */}
								{event.target && (
									<div>
										<Text size="xs" c="dimmed" tt="uppercase" mb="xs">
											Target
										</Text>
										<Stack gap="xs">
											{event.target.resourceType && (
												<DetailRow
													label="Resource Type"
													value={event.target.resourceType}
												/>
											)}
											{event.target.resourceId && (
												<DetailRow
													label="Resource ID"
													value={event.target.resourceId}
													copyable
												/>
											)}
											{event.target.reference && (
												<DetailRow
													label="Reference"
													value={event.target.reference}
													copyable
												/>
											)}
											{event.target.query && (
												<DetailRow label="Query" value={event.target.query} copyable />
											)}
										</Stack>
									</div>
								)}
							</Stack>
						</Tabs.Panel>

						{hasChanges && (
							<Tabs.Panel value="changes" pt="md">
								<DiffViewer changes={event.changes} />
							</Tabs.Panel>
						)}

						{hasContext && (
							<Tabs.Panel value="context" pt="md">
								<Stack gap="xs">
									{Object.entries(event.context || {}).map(([key, value]) => (
										<DetailRow
											key={key}
											label={key
												.replace(/([A-Z])/g, " $1")
												.replace(/^./, (s) => s.toUpperCase())}
											value={String(value)}
											copyable={typeof value === "string"}
										/>
									))}
								</Stack>
							</Tabs.Panel>
						)}

						<Tabs.Panel value="raw" pt="md">
							<Paper withBorder p="sm">
								<ScrollArea.Autosize mah={400}>
									<Code block size="xs">
										{JSON.stringify(event, null, 2)}
									</Code>
								</ScrollArea.Autosize>
							</Paper>
						</Tabs.Panel>
					</Tabs>
				</Stack>
			</ScrollArea>
		</div>
	);
}

export const AuditEventDetail = memo(AuditEventDetailComponent);
