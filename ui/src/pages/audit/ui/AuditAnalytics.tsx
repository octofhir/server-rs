import { memo, useMemo } from "react";
import {
	Text,
	Progress,
	Badge,
	ThemeIcon,
	Center,
	RingProgress,
	Skeleton,
	EmptyState,
} from "@octofhir/ui-kit";
import { User as Person, Database, Check, X as Xmark, TriangleAlert as TriangleExclamation, TrendingUp as ChartLineArrowUp, Clock } from "lucide-react";
import type { AuditAnalytics as AuditAnalyticsType, AuditAction, AuditOutcome } from "@/shared/api/types";
import {
	getAuditActionColor,
	getAuditActionLabel,
	getAuditOutcomeColor,
	getAuditOutcomeLabel,
	isAuditAction,
} from "@/entities/audit-event";
import classes from "./AuditAnalytics.module.css";

interface AuditAnalyticsProps {
	analytics: AuditAnalyticsType | undefined;
	isLoading: boolean;
	isError?: boolean;
	error?: unknown;
	onRetry?: () => void;
}

function StatCard({
	title,
	value,
	icon: Icon,
	color = "primary",
	trend,
}: {
	title: string;
	value: string | number;
	icon: typeof Person;
	color?: string;
	trend?: { value: number; label: string };
}) {
	return (
		<div className={classes.statCard}>
			<div className={classes.statHeader}>
				<div className={classes.statMeta}>
					<Text variant="caption-2" color="secondary" className={classes.statTitle}>
						{title}
					</Text>
					<Text variant="header-1">
						{typeof value === "number" ? value.toLocaleString() : value}
					</Text>
					{trend && (
						<div className={classes.trend}>
							<ChartLineArrowUp
								width={12}
								height={12}
								aria-hidden="true"
								color={trend.value >= 0 ? "var(--g-color-base-positive-medium-hover)" : "var(--g-color-base-danger-medium)"}
							/>
							<Text variant="caption-2" color={trend.value >= 0 ? "positive" : "danger"}>
								{trend.value > 0 ? "+" : ""}{trend.value}% {trend.label}
							</Text>
						</div>
					)}
				</div>
				<ThemeIcon size="lg" view="light" color={color} radius="md">
					<Icon width={20} height={20} aria-hidden="true" />
				</ThemeIcon>
			</div>
		</div>
	);
}

function OutcomeRing({ outcomeBreakdown }: { outcomeBreakdown: Partial<Record<AuditOutcome, number>> }) {
	const total = Object.values(outcomeBreakdown).reduce((a, b) => (a ?? 0) + (b ?? 0), 0) ?? 0;
	if (total === 0) return null;

	const success = outcomeBreakdown.success ?? 0;
	const failure = outcomeBreakdown.failure ?? 0;
	const partial = outcomeBreakdown.partial ?? 0;

	const sections = [
		{
			value: (success / total) * 100,
			color: "var(--g-color-base-positive-medium)",
			tooltip: `Success: ${success.toLocaleString()}`,
		},
		{
			value: (failure / total) * 100,
			color: "var(--g-color-base-danger-medium)",
			tooltip: `Failure: ${failure.toLocaleString()}`,
		},
		{
			value: (partial / total) * 100,
			color: "var(--g-color-base-warning-medium)",
			tooltip: `Partial: ${partial.toLocaleString()}`,
		},
	].filter((s) => s.value > 0);

	return (
		<div className={classes.chartCard}>
			<Text variant="subheader-1" className={classes.cardTitle}>
				Outcome Distribution
			</Text>
			<div className={classes.ringLayout}>
				<RingProgress
					size={140}
					thickness={16}
					roundCaps
					sections={sections}
					label={
						<Center>
							<div className={classes.ringLabel}>
								<Text variant="subheader-2">
									{total.toLocaleString()}
								</Text>
								<Text variant="caption-2" color="secondary">
									Total
								</Text>
							</div>
						</Center>
					}
				/>
				<div className={classes.legend}>
					{(["success", "failure", "partial"] as const).map((outcome) => {
						const count = outcomeBreakdown[outcome] || 0;
						const percent = total > 0 ? ((count / total) * 100).toFixed(1) : "0";
						const Icon = outcome === "success" ? Check : outcome === "failure" ? Xmark : TriangleExclamation;

						return (
							<div key={outcome} className={classes.legendRow}>
								<ThemeIcon size="xs" color={getAuditOutcomeColor(outcome)} view="filled">
									<Icon width={10} height={10} aria-hidden="true" />
								</ThemeIcon>
								<Text variant="body-2" className={classes.legendLabel}>
									{getAuditOutcomeLabel(outcome)}
								</Text>
								<Text variant="body-2" color="secondary">
									{count.toLocaleString()} ({percent}%)
								</Text>
							</div>
						);
					})}
				</div>
			</div>
		</div>
	);
}

function ActionBreakdown({ actionBreakdown }: { actionBreakdown: Partial<Record<AuditAction, number>> }) {
	const sorted = useMemo(() => {
		return Object.entries(actionBreakdown)
			.sort(([, a], [, b]) => b - a)
			.slice(0, 8);
	}, [actionBreakdown]);

	const max = Math.max(...sorted.map(([, count]) => count));

	if (sorted.length === 0) return null;

	return (
		<div className={classes.chartCard}>
			<Text variant="subheader-1" className={classes.cardTitle}>
				Actions by Type
			</Text>
			<div className={classes.breakdownList}>
				{sorted.map(([action, count]) => {
					const actionColor = isAuditAction(action) ? getAuditActionColor(action) : "gray";
					const actionLabel = isAuditAction(action) ? getAuditActionLabel(action) : action;

					return (
						<div key={action} className={classes.breakdownItem}>
							<div className={classes.breakdownHeader}>
								<Badge size="sm" color={actionColor}>
									{actionLabel}
								</Badge>
								<Text variant="caption-2" color="secondary">
									{count.toLocaleString()}
								</Text>
							</div>
							<Progress
								value={(count / max) * 100}
								size="sm"
								color={actionColor}
							/>
						</div>
					);
				})}
			</div>
		</div>
	);
}

function TopUsers({ topUsers }: { topUsers: AuditAnalyticsType["topUsers"] }) {
	if (!topUsers || topUsers.length === 0) return null;

	const max = Math.max(...topUsers.map((u) => u.count));

	return (
		<div className={classes.chartCard}>
			<Text size="sm" fw={600} mb="md">
				Top Users by Activity
			</Text>
			<div className={classes.breakdownList}>
				{topUsers.slice(0, 5).map((user, index) => (
					<div key={user.userId} className={classes.breakdownItem}>
						<div className={classes.breakdownHeader}>
							<div className={classes.rankLabel}>
								<Badge size="xs" variant="filled" color="gray" circle>
									{index + 1}
								</Badge>
								<Text size="sm">
									{user.userName || user.userId}
								</Text>
							</div>
							<Text size="xs" c="dimmed">
								{user.count.toLocaleString()} events
							</Text>
						</div>
						<Progress
							value={(user.count / max) * 100}
							size="sm"
							color="blue"
							radius="sm"
						/>
					</div>
				))}
			</div>
		</div>
	);
}

function TopResources({ topResources }: { topResources: AuditAnalyticsType["topResources"] }) {
	if (!topResources || topResources.length === 0) return null;

	const max = Math.max(...topResources.map((r) => r.count));

	return (
		<div className={classes.chartCard}>
			<Text size="sm" fw={600} mb="md">
				Most Accessed Resources
			</Text>
			<div className={classes.breakdownList}>
				{topResources.slice(0, 5).map((resource, index) => (
					<div key={`${resource.resourceType}-${resource.resourceId || index}`} className={classes.breakdownItem}>
						<div className={classes.breakdownHeader}>
							<div className={classes.rankLabel}>
								<Badge size="xs" variant="filled" color="gray" circle>
									{index + 1}
								</Badge>
								<Text size="sm">
									{resource.resourceType}
									{resource.resourceId && `/${resource.resourceId.slice(0, 8)}...`}
								</Text>
							</div>
							<Text size="xs" c="dimmed">
								{resource.count.toLocaleString()} events
							</Text>
						</div>
						<Progress
							value={(resource.count / max) * 100}
							size="sm"
							color="violet"
							radius="sm"
						/>
					</div>
				))}
			</div>
		</div>
	);
}

function FailedAttempts({ failedAttempts }: { failedAttempts: AuditAnalyticsType["failedAttempts"] }) {
	if (!failedAttempts || failedAttempts.length === 0) return null;

	return (
		<div className={classes.chartCard}>
			<Text size="sm" fw={600} mb="md">
				Failed Action Attempts
			</Text>
			<div className={classes.breakdownList}>
				{failedAttempts.slice(0, 5).map((attempt) => (
					<div key={attempt.action} className={classes.failedRow}>
						<div className={classes.rankLabel}>
							<ThemeIcon size="sm" color="red" variant="light">
								<Xmark size={12} />
							</ThemeIcon>
							<Text size="sm">{getAuditActionLabel(attempt.action)}</Text>
						</div>
						<div className={classes.failedMeta}>
							<Badge size="sm" color="red" variant="light">
								{attempt.count} failures
							</Badge>
							<Text size="xs" c="dimmed">
								{new Date(attempt.lastAttempt).toLocaleDateString()}
							</Text>
						</div>
					</div>
				))}
			</div>
		</div>
	);
}

function ActivityTimeline({ activityOverTime }: { activityOverTime: AuditAnalyticsType["activityOverTime"] }) {
	if (!activityOverTime || activityOverTime.length === 0) return null;

	const max = Math.max(...activityOverTime.map((p) => p.count));

	return (
		<div className={classes.chartCard}>
			<Text size="sm" fw={600} mb="md">
				Activity Over Time
			</Text>
			<div className={classes.timeline}>
				{activityOverTime.slice(-24).map((point) => {
					const height = max > 0 ? (point.count / max) * 100 : 0;
					const date = new Date(point.timestamp);
					const level = Math.max(1, Math.ceil(height / 10));
					const barClassName = `${classes.timelineBar} ${classes[`timelineBar${level}`]}`;

					return (
						<div
							key={point.timestamp}
							className={barClassName}
							title={`${date.toLocaleString()}: ${point.count} events`}
						/>
					);
				})}
			</div>
			<div className={classes.timelineLabels}>
				<Text size="xs" c="dimmed">
					{activityOverTime.length > 0
						? new Date(activityOverTime[0].timestamp).toLocaleDateString()
						: ""}
				</Text>
				<Text size="xs" c="dimmed">
					{activityOverTime.length > 0
						? new Date(activityOverTime[activityOverTime.length - 1].timestamp).toLocaleDateString()
						: ""}
				</Text>
			</div>
		</div>
	);
}

function AuditAnalyticsComponent({ analytics, isLoading }: AuditAnalyticsProps) {
	if (isLoading) {
		return (
			<Center py="xl">
				<Loader size="lg" />
			</Center>
		);
	}

	if (!analytics) {
		return (
			<Center py="xl">
				<div className={classes.emptyState}>
					<ThemeIcon size={48} variant="light" color="gray" radius="xl">
						<ChartLineArrowUp size={24} />
					</ThemeIcon>
					<Text c="dimmed">No analytics data available</Text>
				</div>
			</Center>
		);
	}

	const outcomeBreakdown = analytics.outcomeBreakdown ?? {};
	const actionBreakdown = analytics.actionBreakdown ?? {};
	const topUsers = analytics.topUsers ?? [];
	const topResources = analytics.topResources ?? [];
	const failedAttempts = analytics.failedAttempts ?? [];
	const activityOverTime = analytics.activityOverTime ?? [];

	const totalEvents = Object.values(outcomeBreakdown).reduce((a, b) => a + b, 0);
	const successRate = totalEvents > 0
		? (((outcomeBreakdown.success ?? 0) / totalEvents) * 100).toFixed(1)
		: "0";

	return (
		<div className={classes.root}>
			{/* Summary Stats */}
			<div className={classes.statsGrid}>
				<StatCard
					title="Total Events"
					value={totalEvents}
					icon={Clock}
					color="blue"
				/>
				<StatCard
					title="Success Rate"
					value={`${successRate}%`}
					icon={Check}
					color="green"
				/>
				<StatCard
					title="Active Users"
					value={topUsers.length}
					icon={Person}
					color="violet"
				/>
				<StatCard
					title="Resources Accessed"
					value={topResources.length}
					icon={Database}
					color="orange"
				/>
			</div>

			{/* Activity Timeline */}
			<ActivityTimeline activityOverTime={activityOverTime} />

			{/* Charts Grid */}
			<div className={classes.chartGrid}>
				<OutcomeRing outcomeBreakdown={outcomeBreakdown} />
				<ActionBreakdown actionBreakdown={actionBreakdown} />
				<TopUsers topUsers={topUsers} />
				<TopResources topResources={topResources} />
			</div>

			{/* Failed Attempts */}
			{failedAttempts.length > 0 && (
				<FailedAttempts failedAttempts={failedAttempts} />
			)}
		</div>
	);
}

export const AuditAnalytics = memo(AuditAnalyticsComponent);
