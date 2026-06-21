import { memo } from "react";
import {
	Text,
	Badge,
	Divider,
	Tabs,
	Code,
	ThemeIcon,
	ScrollArea,
	ClipboardButton,
} from "@octofhir/ui-kit";
import { User as Person, Server, Monitor as Display, Clock, GitBranch as BranchesRight, Check, X as Xmark, TriangleAlert as TriangleExclamation, Plus, Minus, ArrowRight, ArrowRightLeft as ArrowRightArrowLeft } from "lucide-react";
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
				<ThemeIcon size="sm" view="light" color="gray">
					<Icon width={12} height={12} aria-hidden="true" />
				</ThemeIcon>
			)}
			<Text variant="body-2" color="secondary" className={classes.detailLabel}>
				{label}
			</Text>
			<div className={classes.detailValueGroup}>
				<Text variant="body-2" className={classes.detailValue}>
					{value}
				</Text>
				{copyable && (
					<ClipboardButton
						text={value}
						size="xs"
						variant="subtle"
						aria-label={`Copy ${label}`}
						tooltipInitialText={`Copy ${label}`}
					/>
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
								color={
									change.op === "add"
										? "green"
										: change.op === "remove"
											? "red"
											: "yellow"
								}
								leftSection={
									change.op === "add" ? (
										<Plus width={10} height={10} aria-hidden="true" />
									) : change.op === "remove" ? (
										<Minus width={10} height={10} aria-hidden="true" />
									) : (
										<ArrowRightArrowLeft width={10} height={10} aria-hidden="true" />
									)
								}
							>
								{change.op}
							</Badge>
							<Code className={classes.codeXs}>{change.path}</Code>
						</div>
						<div className={classes.diffColumns}>
							{change.op !== "add" && change.oldValue !== undefined && (
								<div className={classes.diffOld}>
									<Text variant="caption-2" color="secondary" className={classes.diffColumnLabel}>
										Before
									</Text>
									<Code block className={classes.codeXs}>
										{JSON.stringify(change.oldValue, null, 2)}
									</Code>
								</div>
							)}
							{change.op !== "remove" && change.newValue !== undefined && (
								<div className={classes.diffNew}>
									<Text variant="caption-2" color="secondary" className={classes.diffColumnLabel}>
										After
									</Text>
									<Code block className={classes.codeXs}>
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
						<Text variant="body-2" className={classes.diffColumnHeading}>
							Before
						</Text>
						<ScrollArea.Autosize mah={300}>
							<Code block className={classes.codeXs}>
								{JSON.stringify(before, null, 2)}
							</Code>
						</ScrollArea.Autosize>
					</div>
				)}
				{before && after && (
					<ThemeIcon size="lg" view="light" color="gray" className={classes.diffArrow}>
						<ArrowRight width={16} height={16} aria-hidden="true" />
					</ThemeIcon>
				)}
				{after && (
					<div className={classes.diffNew}>
						<Text variant="body-2" className={classes.diffColumnHeading}>
							After
						</Text>
						<ScrollArea.Autosize mah={300}>
							<Code block className={classes.codeXs}>
								{JSON.stringify(after, null, 2)}
							</Code>
						</ScrollArea.Autosize>
					</div>
				)}
			</div>
		);
	}

	return <Text color="secondary" variant="body-2">No changes recorded</Text>;
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
								<Text variant="subheader-2">
									{getAuditActionDetailLabel(event.action)}
								</Text>
								<div className={classes.badgeRow}>
									<Badge
										size="lg"
										color={getAuditOutcomeColor(event.outcome)}
										leftSection={<OutcomeIcon width={12} height={12} aria-hidden="true" />}
									>
										{getAuditOutcomeLabel(event.outcome)}
									</Badge>
									{event.outcomeDescription && (
										<Text variant="body-2" color="secondary">
											{event.outcomeDescription}
										</Text>
									)}
								</div>
							</div>
							<div className={classes.eventIdGroup}>
								<Badge size="sm" color="gray">
									{event.id.slice(0, 8)}...
								</Badge>
								<ClipboardButton
									text={event.id}
									size="xs"
									variant="subtle"
									aria-label="Copy Event ID"
									tooltipInitialText="Copy Event ID"
								/>
							</div>
						</div>
					</div>

					<Divider />

					{/* Tabs */}
					<Tabs defaultValue="details">
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
									<Text variant="caption-2" color="secondary" className={classes.sectionLabel}>
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
									<Text variant="caption-2" color="secondary" className={classes.sectionLabel}>
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
									<Text variant="caption-2" color="secondary" className={classes.sectionLabel}>
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
										<Text variant="caption-2" color="secondary" className={classes.sectionLabel}>
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
									<Code block className={classes.codeXs}>
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
