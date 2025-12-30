import { useState, useCallback, useMemo } from "react";
import {
	Box,
	Stack,
	Title,
	Text,
	Group,
	ThemeIcon,
	Tabs,
	ActionIcon,
	Tooltip,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconShield, IconList, IconChartBar, IconX } from "@tabler/icons-react";
import { useAuditEvents, useAuditAnalytics, exportAuditLogs } from "./lib/useAudit";
import { AuditFilters } from "./ui/AuditFilters";
import { AuditEventList } from "./ui/AuditEventList";
import { AuditEventDetail } from "./ui/AuditEventDetail";
import { AuditAnalytics } from "./ui/AuditAnalytics";
import type { AuditEvent, AuditEventUIFilters } from "@/shared/api/types";
import classes from "./AuditTrailPage.module.css";

export function AuditTrailPage() {
	const [activeTab, setActiveTab] = useState<string | null>("events");
	const [filters, setFilters] = useState<AuditEventUIFilters>({});
	const [selectedEvent, setSelectedEvent] = useState<AuditEvent | null>(null);

	// Fetch audit events with infinite scroll
	const {
		data: eventsData,
		isLoading: isLoadingEvents,
		isFetchingNextPage,
		hasNextPage,
		fetchNextPage,
		refetch: refetchEvents,
	} = useAuditEvents(filters);

	// Fetch analytics with time range from filters
	const timeRange = useMemo(() => {
		if (filters.startTime || filters.endTime) {
			return {
				start: filters.startTime || new Date(0).toISOString(),
				end: filters.endTime || new Date().toISOString(),
			};
		}
		return undefined;
	}, [filters.startTime, filters.endTime]);

	const {
		data: analytics,
		isLoading: isLoadingAnalytics,
	} = useAuditAnalytics(timeRange);

	// Flatten events from infinite query pages
	const events = useMemo(() => {
		return eventsData?.pages.flatMap((page) => page.events) || [];
	}, [eventsData]);

	const totalCount = useMemo(() => {
		return eventsData?.pages[0]?.total || 0;
	}, [eventsData]);

	const handleFiltersChange = useCallback((newFilters: Partial<AuditEventUIFilters>) => {
		setFilters((prev) => ({ ...prev, ...newFilters }));
		setSelectedEvent(null);
	}, []);

	const handleRefresh = useCallback(() => {
		refetchEvents();
	}, [refetchEvents]);

	const handleExport = useCallback(async (format: "json" | "csv") => {
		try {
			await exportAuditLogs(filters, format);
			notifications.show({
				title: "Export successful",
				message: `Audit logs exported as ${format.toUpperCase()}`,
				color: "green",
			});
		} catch (error) {
			notifications.show({
				title: "Export failed",
				message: error instanceof Error ? error.message : "Unknown error",
				color: "red",
			});
		}
	}, [filters]);

	const handleEventClick = useCallback((event: AuditEvent) => {
		setSelectedEvent(event);
	}, []);

	const handleLoadMore = useCallback(() => {
		if (hasNextPage && !isFetchingNextPage) {
			fetchNextPage();
		}
	}, [hasNextPage, isFetchingNextPage, fetchNextPage]);

	const handleCloseDetail = useCallback(() => {
		setSelectedEvent(null);
	}, []);

	return (
		<Box className={`${classes.container} page-enter`}>
			<Stack gap={0} className={classes.stack}>
				{/* Header */}
				<Box className={classes.header}>
					<Group justify="space-between" align="flex-start">
						<Group gap="md">
							<ThemeIcon variant="light" color="primary" size={48} radius="md">
								<IconShield size={24} />
							</ThemeIcon>
							<div>
								<Title order={2} style={{ letterSpacing: "-0.02em" }}>
									Audit Trail
								</Title>
								<Text c="dimmed" size="sm">
									Track and analyze all system activity and changes
								</Text>
							</div>
						</Group>
					</Group>
				</Box>

				{/* Tabs */}
				<Tabs
					value={activeTab}
					onChange={setActiveTab}
					className={classes.tabs}
				>
					<Tabs.List className={classes.tabsList}>
						<Tabs.Tab value="events" leftSection={<IconList size={16} />}>
							Events
						</Tabs.Tab>
						<Tabs.Tab value="analytics" leftSection={<IconChartBar size={16} />}>
							Analytics
						</Tabs.Tab>
					</Tabs.List>
				</Tabs>

				{/* Content */}
				{activeTab === "events" && (
					<>
						<AuditFilters
							filters={filters}
							totalCount={totalCount}
							isLoading={isLoadingEvents}
							onFiltersChange={handleFiltersChange}
							onRefresh={handleRefresh}
							onExport={handleExport}
						/>

						<div className={classes.content}>
							<div
								className={classes.listContainer}
								data-has-detail={!!selectedEvent}
							>
								<AuditEventList
									events={events}
									isLoading={isLoadingEvents}
									isFetchingNextPage={isFetchingNextPage}
									hasNextPage={hasNextPage ?? false}
									selectedEventId={selectedEvent?.id}
									onEventClick={handleEventClick}
									onLoadMore={handleLoadMore}
								/>
							</div>

							{selectedEvent && (
								<div className={classes.detailContainer}>
									<div className={classes.detailHeader}>
										<Text size="sm" fw={600}>
											Event Details
										</Text>
										<Tooltip label="Close">
											<ActionIcon
												variant="subtle"
												color="gray"
												size="sm"
												onClick={handleCloseDetail}
											>
												<IconX size={14} />
											</ActionIcon>
										</Tooltip>
									</div>
									<AuditEventDetail event={selectedEvent} />
								</div>
							)}
						</div>
					</>
				)}

				{activeTab === "analytics" && (
					<Box className={classes.analyticsContainer}>
						<AuditAnalytics
							analytics={analytics}
							isLoading={isLoadingAnalytics}
						/>
					</Box>
				)}
			</Stack>
		</Box>
	);
}
