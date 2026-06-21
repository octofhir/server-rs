import { ActionIcon, Tabs, Text, Tooltip, notify } from "@octofhir/ui-kit";
import { useState, useCallback, useMemo } from "react";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { List as ListUl, BarChart3 as ChartBar, X as Xmark } from "lucide-react";
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
		isError: isEventsError,
		error: eventsError,
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
		isError: isAnalyticsError,
		error: analyticsError,
		refetch: refetchAnalytics,
	} = useAuditAnalytics(timeRange);

	const hasActiveFilters =
		(filters.action?.length ?? 0) > 0 ||
		(filters.outcome?.length ?? 0) > 0 ||
		(filters.actorType?.length ?? 0) > 0 ||
		Boolean(filters.startTime) ||
		Boolean(filters.endTime) ||
		Boolean(filters.resourceType) ||
		Boolean(filters.ipAddress) ||
		Boolean(filters.search);

	const clearAllFilters = useCallback(() => {
		setFilters({});
		setSelectedEvent(null);
	}, []);

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
			notify({
				theme: "success",
				title: "Export successful",
				content: `Audit logs exported as ${format.toUpperCase()}`,
			});
		} catch (error) {
			notify({
				theme: "danger",
				title: "Export failed",
				content: error instanceof Error ? error.message : "Unknown error",
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
		<WorkspacePageLayout
			title="Audit Trail"
			description="Track and analyze all system activity and changes"
			className="page-enter"
			bodyClassName={classes.body}
			contentClassName={classes.container}
		>
			<div className={classes.stack}>
				<Tabs
					value={activeTab}
					onChange={setActiveTab}
					className={classes.tabs}
				>
					<Tabs.List className={classes.tabsList}>
						<Tabs.Tab value="events" leftSection={<ListUl width={16} height={16} aria-hidden="true" />}>
							Events
						</Tabs.Tab>
						<Tabs.Tab value="analytics" leftSection={<ChartBar width={16} height={16} aria-hidden="true" />}>
							Analytics
						</Tabs.Tab>
					</Tabs.List>
				</Tabs>

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
									isError={isEventsError}
									error={eventsError}
									onRetry={refetchEvents}
									hasActiveFilters={hasActiveFilters}
									onClearFilters={clearAllFilters}
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
										<Text variant="subheader-1">Event Details</Text>
										<Tooltip label="Close">
											<ActionIcon
												variant="subtle"
												size="sm"
												aria-label="Close event details"
												onClick={handleCloseDetail}
											>
												<Xmark width={14} height={14} aria-hidden="true" />
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
					<div className={classes.analyticsContainer}>
						<AuditAnalytics
							analytics={analytics}
							isLoading={isLoadingAnalytics}
							isError={isAnalyticsError}
							error={analyticsError}
							onRetry={refetchAnalytics}
						/>
					</div>
				)}
			</div>
		</WorkspacePageLayout>
	);
}
