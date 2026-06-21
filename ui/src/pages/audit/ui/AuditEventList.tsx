import { memo, useEffect, useRef, useCallback } from "react";
import {
	Table,
	Text,
	Badge,
	Spin,
	Center,
	Tooltip,
	ThemeIcon,
	Skeleton,
	EmptyState,
} from "@octofhir/ui-kit";
import {
	Person,
	Server,
	Display,
	ArrowRightToSquare,
	ArrowRightFromSquare,
	Plus,
	Eye,
	Pencil,
	TrashBin,
	Magnifier,
	Shield,
	Gear,
	Power,
	Xmark,
	Check,
	TriangleExclamation,
} from "@gravity-ui/icons";
import type { AuditEvent, AuditAction, AuditOutcome } from "@/shared/api/types";
import {
	getAuditActionColor,
	getAuditActionLabel,
	getAuditActorLabel,
	getAuditOutcomeColor,
	getAuditTargetView,
	getAuditTimestampView,
} from "@/entities/audit-event";
import classes from "./AuditEventList.module.css";

interface AuditEventListProps {
	events: AuditEvent[];
	isLoading: boolean;
	isError?: boolean;
	error?: unknown;
	onRetry?: () => void;
	hasActiveFilters?: boolean;
	onClearFilters?: () => void;
	isFetchingNextPage: boolean;
	hasNextPage: boolean;
	selectedEventId?: string;
	onEventClick: (event: AuditEvent) => void;
	onLoadMore: () => void;
}

function getActionIcon(action: AuditAction) {
	const icons: Record<AuditAction, typeof Person> = {
		"user.login": ArrowRightToSquare,
		"user.logout": ArrowRightFromSquare,
		"user.login_failed": Xmark,
		"resource.create": Plus,
		"resource.read": Eye,
		"resource.update": Pencil,
		"resource.delete": TrashBin,
		"resource.search": Magnifier,
		"policy.evaluate": Shield,
		"client.auth": Display,
		"client.create": Plus,
		"client.update": Pencil,
		"client.delete": TrashBin,
		"config.change": Gear,
		"system.startup": Power,
		"system.shutdown": Power,
	};
	return icons[action] || Server;
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

function AuditEventListComponent({
	events,
	isLoading,
	isError,
	error,
	onRetry,
	hasActiveFilters,
	onClearFilters,
	isFetchingNextPage,
	hasNextPage,
	selectedEventId,
	onEventClick,
	onLoadMore,
}: AuditEventListProps) {
	const containerRef = useRef<HTMLDivElement>(null);
	const loadMoreRef = useRef<HTMLDivElement>(null);

	const handleIntersection = useCallback(
		(entries: IntersectionObserverEntry[]) => {
			const [entry] = entries;
			if (entry.isIntersecting && hasNextPage && !isFetchingNextPage) {
				onLoadMore();
			}
		},
		[hasNextPage, isFetchingNextPage, onLoadMore]
	);

	useEffect(() => {
		const observer = new IntersectionObserver(handleIntersection, {
			root: containerRef.current, // Use scroll container as root
			rootMargin: "100px",
			threshold: 0,
		});

		if (loadMoreRef.current) {
			observer.observe(loadMoreRef.current);
		}

		return () => observer.disconnect();
	}, [handleIntersection]);

	if (isLoading && events.length === 0) {
		return (
			<output className={classes.skeletonList} aria-busy="true" aria-label="Loading audit events">
				{Array.from({ length: 8 }).map((_, i) => (
					// biome-ignore lint/suspicious/noArrayIndexKey: static skeleton placeholders
					<Skeleton key={i} className={classes.skeletonRow} />
				))}
			</output>
		);
	}

	if (isError) {
		return (
			<div className={classes.stateWrap}>
				<EmptyState
					image={<TriangleExclamation width={48} height={48} aria-hidden="true" />}
					title="Failed to load audit events"
					description={error instanceof Error ? error.message : "An unexpected error occurred."}
					actions={
						onRetry ? [{ text: "Retry", view: "action", onClick: onRetry }] : undefined
					}
				/>
			</div>
		);
	}

	if (events.length === 0) {
		return (
			<div className={classes.stateWrap}>
				<EmptyState
					image={<Shield width={48} height={48} aria-hidden="true" />}
					title={hasActiveFilters ? "No matching audit events" : "No audit events yet"}
					description={
						hasActiveFilters
							? "No events match the current filters. Try clearing them to see all activity."
							: "System activity will appear here as it is recorded."
					}
					actions={
						hasActiveFilters && onClearFilters
							? [{ text: "Clear filters", view: "normal", onClick: onClearFilters }]
							: undefined
					}
				/>
			</div>
		);
	}

	return (
		<div ref={containerRef} className={classes.container}>
			<Table highlightOnHover className={classes.table}>
				<Table.Thead className={classes.thead}>
					<Table.Tr>
						<Table.Th className={classes.timeCell}>Time</Table.Th>
						<Table.Th className={classes.actionCell}>Action</Table.Th>
						<Table.Th className={classes.outcomeCell}>Outcome</Table.Th>
						<Table.Th>Actor</Table.Th>
						<Table.Th>Target</Table.Th>
						<Table.Th className={classes.sourceCell}>Source</Table.Th>
					</Table.Tr>
				</Table.Thead>
				<Table.Tbody>
					{events.map((event) => {
						const ActionIcon = getActionIcon(event.action);
						const OutcomeIcon = getOutcomeIcon(event.outcome);
						const ActorIcon = getActorIcon(event.actor.type);
						const timestamp = getAuditTimestampView(event.timestamp);
						const target = getAuditTargetView(event);
						const isSelected = event.id === selectedEventId;

						return (
							<Table.Tr
								key={event.id}
								onClick={() => onEventClick(event)}
								className={classes.row}
								data-selected={isSelected}
							>
								<Table.Td>
									<Tooltip content={timestamp.full}>
										<div className={classes.cellStack}>
											<Text variant="body-2" className={classes.strongText}>
												{timestamp.time}
											</Text>
											<Text variant="caption-2" color="secondary">
												{timestamp.relative}
											</Text>
										</div>
									</Tooltip>
								</Table.Td>
								<Table.Td>
									<div className={classes.inlineCell}>
										<ThemeIcon
											size="s"
											view="light"
											color={getAuditActionColor(event.action)}
										>
											<ActionIcon width={12} height={12} aria-hidden="true" />
										</ThemeIcon>
										<Text variant="body-2">{getAuditActionLabel(event.action)}</Text>
									</div>
								</Table.Td>
								<Table.Td>
									<Badge
										size="sm"
										color={getAuditOutcomeColor(event.outcome)}
										leftSection={<OutcomeIcon width={10} height={10} aria-hidden="true" />}
									>
										{event.outcome}
									</Badge>
								</Table.Td>
								<Table.Td>
									<div className={classes.inlineCell}>
										<ThemeIcon size="s" view="subtle" color="gray">
											<ActorIcon width={12} height={12} aria-hidden="true" />
										</ThemeIcon>
										<div className={classes.cellStack}>
											<Text variant="body-2" className={classes.truncateText}>
												{getAuditActorLabel(event)}
											</Text>
											{event.actor.name && event.actor.id && (
												<Text variant="caption-2" color="secondary" className={classes.truncateText}>
													{event.actor.id}
												</Text>
											)}
										</div>
									</div>
								</Table.Td>
								<Table.Td>
									{target ? (
										<div className={classes.cellStack}>
											<Text variant="body-2" className={classes.truncateText}>
												{target.primary}
											</Text>
											{target.secondary && (
												<Text variant="caption-2" color="secondary" className={classes.truncateText}>
													{target.secondary}
												</Text>
											)}
										</div>
									) : (
										<Text variant="body-2" color="secondary">
											—
										</Text>
									)}
								</Table.Td>
								<Table.Td>
									<Tooltip content={event.source.userAgent || "Unknown"}>
										<Text variant="caption-2" color="secondary" className={classes.truncateText}>
											{event.source.ipAddress || "—"}
										</Text>
									</Tooltip>
								</Table.Td>
							</Table.Tr>
						);
					})}
				</Table.Tbody>
			</Table>

			<div ref={loadMoreRef} className={classes.loadMore}>
				{isFetchingNextPage && (
					<Center py="md">
						<Spin size="s" />
					</Center>
				)}
			</div>
		</div>
	);
}

export const AuditEventList = memo(AuditEventListComponent);
