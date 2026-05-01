import { memo, useEffect, useRef, useCallback } from "react";
import {
	Table,
	Text,
	Badge,
	Group,
	Stack,
	Loader,
	Center,
	Tooltip,
	ThemeIcon,
} from "@/shared/ui";
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
			<Center py="xl">
				<Loader size="lg" />
			</Center>
		);
	}

	if (events.length === 0) {
		return (
			<Center py="xl">
				<Stack align="center" gap="sm">
					<ThemeIcon size={48} variant="light" color="gray" radius="xl">
						<Shield size={24} />
					</ThemeIcon>
					<Text c="dimmed">No audit events found</Text>
					<Text size="xs" c="dimmed">
						Try adjusting your filters
					</Text>
				</Stack>
			</Center>
		);
	}

	return (
		<div ref={containerRef} className={classes.container}>
			<Table highlightOnHover className={classes.table}>
				<Table.Thead className={classes.thead}>
					<Table.Tr>
						<Table.Th style={{ width: 180 }}>Time</Table.Th>
						<Table.Th style={{ width: 140 }}>Action</Table.Th>
						<Table.Th style={{ width: 80 }}>Outcome</Table.Th>
						<Table.Th>Actor</Table.Th>
						<Table.Th>Target</Table.Th>
						<Table.Th style={{ width: 120 }}>Source</Table.Th>
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
									<Tooltip label={timestamp.full}>
										<Stack gap={0}>
											<Text size="sm" fw={500}>
												{timestamp.time}
											</Text>
											<Text size="xs" c="dimmed">
												{timestamp.relative}
											</Text>
										</Stack>
									</Tooltip>
								</Table.Td>
								<Table.Td>
									<Group gap="xs" wrap="nowrap">
										<ThemeIcon
											size="sm"
											variant="light"
											color={getAuditActionColor(event.action)}
										>
											<ActionIcon size={12} />
										</ThemeIcon>
										<Text size="sm">{getAuditActionLabel(event.action)}</Text>
									</Group>
								</Table.Td>
								<Table.Td>
									<Badge
										size="sm"
										variant="light"
										color={getAuditOutcomeColor(event.outcome)}
										leftSection={<OutcomeIcon size={10} />}
									>
										{event.outcome}
									</Badge>
								</Table.Td>
								<Table.Td>
									<Group gap="xs" wrap="nowrap">
										<ThemeIcon size="sm" variant="subtle" color="gray">
											<ActorIcon size={12} />
										</ThemeIcon>
										<Stack gap={0}>
											<Text size="sm" lineClamp={1}>
												{getAuditActorLabel(event)}
											</Text>
											{event.actor.name && event.actor.id && (
												<Text size="xs" c="dimmed" lineClamp={1}>
													{event.actor.id}
												</Text>
											)}
										</Stack>
									</Group>
								</Table.Td>
								<Table.Td>
									{target ? (
										<Stack gap={0}>
											<Text size="sm" lineClamp={1}>
												{target.primary}
											</Text>
											{target.secondary && (
												<Text size="xs" c="dimmed" lineClamp={1}>
													{target.secondary}
												</Text>
											)}
										</Stack>
									) : (
										<Text size="sm" c="dimmed">
											—
										</Text>
									)}
								</Table.Td>
								<Table.Td>
									<Tooltip label={event.source.userAgent || "Unknown"}>
										<Text size="xs" c="dimmed" lineClamp={1}>
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
						<Loader size="sm" />
					</Center>
				)}
			</div>
		</div>
	);
}

export const AuditEventList = memo(AuditEventListComponent);
