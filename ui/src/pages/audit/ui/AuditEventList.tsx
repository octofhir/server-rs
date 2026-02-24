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
	IconUser,
	IconServer,
	IconAppWindow,
	IconLogin,
	IconLogout,
	IconPlus,
	IconEye,
	IconPencil,
	IconTrash,
	IconSearch,
	IconShield,
	IconSettings,
	IconPower,
	IconX,
	IconCheck,
	IconAlertTriangle,
} from "@tabler/icons-react";
import type { AuditEvent, AuditAction, AuditOutcome } from "@/shared/api/types";
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
	const icons: Record<AuditAction, typeof IconUser> = {
		"user.login": IconLogin,
		"user.logout": IconLogout,
		"user.login_failed": IconX,
		"resource.create": IconPlus,
		"resource.read": IconEye,
		"resource.update": IconPencil,
		"resource.delete": IconTrash,
		"resource.search": IconSearch,
		"policy.evaluate": IconShield,
		"client.auth": IconAppWindow,
		"client.create": IconPlus,
		"client.update": IconPencil,
		"client.delete": IconTrash,
		"config.change": IconSettings,
		"system.startup": IconPower,
		"system.shutdown": IconPower,
	};
	return icons[action] || IconServer;
}

function getActionColor(action: AuditAction): string {
	if (action.startsWith("user.login_failed")) return "red";
	if (action.includes("delete")) return "red";
	if (action.includes("create")) return "green";
	if (action.includes("update") || action.includes("change")) return "yellow";
	if (action.includes("login")) return "teal";
	if (action.includes("logout")) return "gray";
	return "blue";
}

function getActionLabel(action: AuditAction): string {
	const labels: Record<AuditAction, string> = {
		"user.login": "User Login",
		"user.logout": "User Logout",
		"user.login_failed": "Login Failed",
		"resource.create": "Create",
		"resource.read": "Read",
		"resource.update": "Update",
		"resource.delete": "Delete",
		"resource.search": "Search",
		"policy.evaluate": "Policy Check",
		"client.auth": "Client Auth",
		"client.create": "Client Created",
		"client.update": "Client Updated",
		"client.delete": "Client Deleted",
		"config.change": "Config Change",
		"system.startup": "System Start",
		"system.shutdown": "System Stop",
	};
	return labels[action] || action;
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

function formatTimestamp(timestamp: string): { date: string; time: string } {
	const d = new Date(timestamp);
	return {
		date: d.toLocaleDateString(),
		time: d.toLocaleTimeString(),
	};
}

function formatRelativeTime(timestamp: string): string {
	const now = new Date();
	const then = new Date(timestamp);
	const diff = now.getTime() - then.getTime();
	const seconds = Math.floor(diff / 1000);
	const minutes = Math.floor(seconds / 60);
	const hours = Math.floor(minutes / 60);
	const days = Math.floor(hours / 24);

	if (seconds < 60) return "Just now";
	if (minutes < 60) return `${minutes}m ago`;
	if (hours < 24) return `${hours}h ago`;
	if (days < 7) return `${days}d ago`;
	return then.toLocaleDateString();
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
						<IconShield size={24} />
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
						const { time } = formatTimestamp(event.timestamp);
						const isSelected = event.id === selectedEventId;

						return (
							<Table.Tr
								key={event.id}
								onClick={() => onEventClick(event)}
								className={classes.row}
								data-selected={isSelected}
							>
								<Table.Td>
									<Tooltip label={new Date(event.timestamp).toLocaleString()}>
										<Stack gap={0}>
											<Text size="sm" fw={500}>
												{time}
											</Text>
											<Text size="xs" c="dimmed">
												{formatRelativeTime(event.timestamp)}
											</Text>
										</Stack>
									</Tooltip>
								</Table.Td>
								<Table.Td>
									<Group gap="xs" wrap="nowrap">
										<ThemeIcon
											size="sm"
											variant="light"
											color={getActionColor(event.action)}
										>
											<ActionIcon size={12} />
										</ThemeIcon>
										<Text size="sm">{getActionLabel(event.action)}</Text>
									</Group>
								</Table.Td>
								<Table.Td>
									<Badge
										size="sm"
										variant="light"
										color={getOutcomeColor(event.outcome)}
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
												{event.actor.name || event.actor.id || event.actor.type}
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
									{event.target ? (
										<Stack gap={0}>
											<Text size="sm" lineClamp={1}>
												{event.target.resourceType}
												{event.target.resourceId && `/${event.target.resourceId}`}
											</Text>
											{event.target.query && (
												<Text size="xs" c="dimmed" lineClamp={1}>
													{event.target.query}
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
