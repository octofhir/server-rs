import { memo } from "react";
import {
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
} from "@/shared/ui";
import {
	Person,
	Server,
	Display,
	Clock,
	BranchesRight,
	Check,
	Xmark,
	TriangleExclamation,
	Copy,
	Plus,
	Minus,
	ArrowRight,
	ArrowRightArrowLeft,
} from "@gravity-ui/icons";
import {
	getAuditActionDetailLabel,
	getAuditOutcomeColor,
	getAuditOutcomeLabel,
	getAuditTimestampView,
} from "@/entities/audit-event";
import type { AuditEvent, AuditOutcome } from "@/shared/api/types";
import classes from "./AuditEventDetail.module.css";

interface AuditEventDetailProps {
	event: AuditEvent;
}

function getOutcomeIcon(outcome: AuditOutcome) {
	switch (outcome) {
		case "success":
			return Check;
		case "failure":
			return Xmark;
		case "partial":
			return TriangleExclamation;
	}
}

function getActorIcon(type: "user" | "client" | "system") {
	switch (type) {
		case "user":
			return Person;
		case "client":
			return Display;
		case "system":
			return Server;
	}
}

function DetailRow({
	label,
	value,
	icon: Icon,
	copyable = false,
}: {
	label: string;
	value?: string | null;
	icon?: typeof Person;
	copyable?: boolean;
}) {
	if (!value) return null;

	return (
		<div className={classes.detailRow}>
			{Icon && (
				<ThemeIcon size="sm" variant="light" color="gray">
					<Icon size={12} />
				</ThemeIcon>
			)}
			<Text size="sm" c="dimmed" className={classes.detailLabel}>
				{label}
			</Text>
			<div className={classes.detailValueGroup}>
				<Text size="sm" className={classes.detailValue}>
					{value}
				</Text>
				{copyable && (
					<CopyButton value={value}>
						{({ copied, copy }) => (
							<Tooltip label={copied ? "Copied" : "Copy"}>
								<ActionIcon size="xs" variant="subtle" onClick={copy}>
									{copied ? <Check size={12} /> : <Copy size={12} />}
								</ActionIcon>
							</Tooltip>
						)}
					</CopyButton>
				)}
			</div>
		</div>
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
			<div className={classes.detailSection}>
					{diff.map((change) => (
						<div key={`${change.op}-${change.path}`} className={classes.diffItem}>
						<div className={classes.diffHeader}>
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
										<Plus size={10} />
									) : change.op === "remove" ? (
										<Minus size={10} />
									) : (
										<ArrowRightArrowLeft size={10} />
									)
								}
							>
								{change.op}
							</Badge>
							<Code size="xs">{change.path}</Code>
						</div>
						<div className={classes.diffColumns}>
							{change.op !== "add" && change.oldValue !== undefined && (
								<div className={classes.diffOld}>
									<Text size="xs" c="dimmed" mb={4}>
										Before
									</Text>
									<Code block size="xs">
										{JSON.stringify(change.oldValue, null, 2)}
									</Code>
								</div>
							)}
							{change.op !== "remove" && change.newValue !== undefined && (
								<div className={classes.diffNew}>
									<Text size="xs" c="dimmed" mb={4}>
										After
									</Text>
									<Code block size="xs">
										{JSON.stringify(change.newValue, null, 2)}
									</Code>
								</div>
							)}
						</div>
					</div>
				))}
			</div>
		);
	}

	// Fall back to before/after comparison
	if (before || after) {
		return (
			<div className={classes.diffColumns}>
				{before && (
					<div className={classes.diffOld}>
						<Text size="sm" fw={500} mb="sm">
							Before
						</Text>
						<ScrollArea.Autosize mah={300}>
							<Code block size="xs">
								{JSON.stringify(before, null, 2)}
							</Code>
						</ScrollArea.Autosize>
					</div>
				)}
				{before && after && (
					<ThemeIcon size="lg" variant="light" color="gray" className={classes.diffArrow}>
						<ArrowRight size={16} />
					</ThemeIcon>
				)}
				{after && (
					<div className={classes.diffNew}>
						<Text size="sm" fw={500} mb="sm">
							After
						</Text>
						<ScrollArea.Autosize mah={300}>
							<Code block size="xs">
								{JSON.stringify(after, null, 2)}
							</Code>
						</ScrollArea.Autosize>
					</div>
				)}
			</div>
		);
	}

	return <Text c="dimmed" size="sm">No changes recorded</Text>;
}

function AuditEventDetailComponent({ event }: AuditEventDetailProps) {
	const OutcomeIcon = getOutcomeIcon(event.outcome);
	const ActorIcon = getActorIcon(event.actor.type);
	const timestamp = getAuditTimestampView(event.timestamp);
	const hasChanges = event.changes && (event.changes.diff || event.changes.before || event.changes.after);
	const hasContext = event.context && Object.keys(event.context).length > 0;

	return (
		<div className={classes.container}>
			<ScrollArea className={classes.scrollArea}>
				<div className={classes.content}>
					{/* Header */}
					<div className={classes.header}>
						<div className={classes.headerRow}>
							<div className={classes.detailSection}>
								<Text size="lg" fw={600}>
									{getAuditActionDetailLabel(event.action)}
								</Text>
								<div className={classes.badgeRow}>
									<Badge
										size="lg"
										variant="light"
										color={getAuditOutcomeColor(event.outcome)}
										leftSection={<OutcomeIcon size={12} />}
									>
										{getAuditOutcomeLabel(event.outcome)}
									</Badge>
									{event.outcomeDescription && (
										<Text size="sm" c="dimmed">
											{event.outcomeDescription}
										</Text>
									)}
								</div>
							</div>
							<CopyButton value={event.id}>
								{({ copied, copy }) => (
									<Tooltip label={copied ? "Copied!" : "Copy Event ID"}>
										<Badge
											size="sm"
											variant="light"
											color="gray"
											className={classes.copyBadge}
											onClick={copy}
										>
											{event.id.slice(0, 8)}...
										</Badge>
									</Tooltip>
								)}
							</CopyButton>
						</div>
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
							<div className={classes.sectionList}>
								{/* Timestamp */}
								<div>
									<Text size="xs" c="dimmed" tt="uppercase" mb="xs">
										Timestamp
									</Text>
									<DetailRow
										label="When"
										value={timestamp.full}
										icon={Clock}
									/>
								</div>

								{/* Actor */}
								<div>
									<Text size="xs" c="dimmed" tt="uppercase" mb="xs">
										Actor
									</Text>
									<div className={classes.detailSection}>
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
									</div>
								</div>

								{/* Source */}
								<div>
									<Text size="xs" c="dimmed" tt="uppercase" mb="xs">
										Source
									</Text>
									<div className={classes.detailSection}>
										<DetailRow
											label="IP Address"
											value={event.source.ipAddress}
											icon={BranchesRight}
											copyable
										/>
										<DetailRow
											label="User Agent"
											value={event.source.userAgent}
											icon={Display}
										/>
										{event.source.site && (
											<DetailRow label="Site" value={event.source.site} />
										)}
									</div>
								</div>

								{/* Target */}
								{event.target && (
									<div>
										<Text size="xs" c="dimmed" tt="uppercase" mb="xs">
											Target
										</Text>
										<div className={classes.detailSection}>
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
										</div>
									</div>
								)}
							</div>
						</Tabs.Panel>

						{hasChanges && (
							<Tabs.Panel value="changes" pt="md">
								<DiffViewer changes={event.changes} />
							</Tabs.Panel>
						)}

						{hasContext && (
							<Tabs.Panel value="context" pt="md">
								<div className={classes.detailSection}>
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
								</div>
							</Tabs.Panel>
						)}

						<Tabs.Panel value="raw" pt="md">
							<div className={classes.rawFrame}>
								<ScrollArea.Autosize mah={400}>
									<Code block size="xs">
										{JSON.stringify(event, null, 2)}
									</Code>
								</ScrollArea.Autosize>
							</div>
						</Tabs.Panel>
					</Tabs>
				</div>
			</ScrollArea>
		</div>
	);
}

export const AuditEventDetail = memo(AuditEventDetailComponent);
