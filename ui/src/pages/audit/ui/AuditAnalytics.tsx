import { memo, useMemo } from "react";
import {
	Stack,
	Group,
	Text,
	Paper,
	SimpleGrid,
	Progress,
	Badge,
	ThemeIcon,
	Center,
	Loader,
	RingProgress,
} from "@/shared/ui";
import {
	IconUser,
	IconDatabase,
	IconCheck,
	IconX,
	IconAlertTriangle,
	IconTrendingUp,
	IconClock,
} from "@tabler/icons-react";
import type { AuditAnalytics as AuditAnalyticsType, AuditAction, AuditOutcome } from "@/shared/api/types";
import classes from "./AuditAnalytics.module.css";

interface AuditAnalyticsProps {
	analytics: AuditAnalyticsType | undefined;
	isLoading: boolean;
}

function getActionLabel(action: AuditAction): string {
	const labels: Record<AuditAction, string> = {
		"user.login": "Login",
		"user.logout": "Logout",
		"user.login_failed": "Login Failed",
		"resource.create": "Create",
		"resource.read": "Read",
		"resource.update": "Update",
		"resource.delete": "Delete",
		"resource.search": "Search",
		"policy.evaluate": "Policy Check",
		"client.auth": "Client Auth",
		"client.create": "Client Create",
		"client.update": "Client Update",
		"client.delete": "Client Delete",
		"config.change": "Config Change",
		"system.startup": "Startup",
		"system.shutdown": "Shutdown",
	};
	return labels[action] || action;
}

function getActionColor(action: AuditAction): string {
	if (action.includes("failed")) return "red";
	if (action.includes("delete")) return "red";
	if (action.includes("create")) return "green";
	if (action.includes("update") || action.includes("change")) return "yellow";
	if (action.includes("login")) return "teal";
	if (action.includes("read") || action.includes("search")) return "blue";
	return "gray";
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

function StatCard({
	title,
	value,
	icon: Icon,
	color = "primary",
	trend,
}: {
	title: string;
	value: string | number;
	icon: typeof IconUser;
	color?: string;
	trend?: { value: number; label: string };
}) {
	return (
		<Paper className={classes.statCard} p="md" withBorder>
			<Group justify="space-between" align="flex-start">
				<div>
					<Text size="xs" c="dimmed" tt="uppercase" mb={4}>
						{title}
					</Text>
					<Text size="xl" fw={700}>
						{typeof value === "number" ? value.toLocaleString() : value}
					</Text>
					{trend && (
						<Group gap={4} mt={4}>
							<IconTrendingUp size={12} color={trend.value >= 0 ? "var(--mantine-color-green-6)" : "var(--mantine-color-red-6)"} />
							<Text size="xs" c={trend.value >= 0 ? "green" : "red"}>
								{trend.value > 0 ? "+" : ""}{trend.value}% {trend.label}
							</Text>
						</Group>
					)}
				</div>
				<ThemeIcon size="lg" variant="light" color={color} radius="md">
					<Icon size={20} />
				</ThemeIcon>
			</Group>
		</Paper>
	);
}

function OutcomeRing({ outcomeBreakdown }: { outcomeBreakdown: Record<AuditOutcome, number> }) {
	const total = Object.values(outcomeBreakdown).reduce((a, b) => a + b, 0);
	if (total === 0) return null;

	const sections = [
		{
			value: (outcomeBreakdown.success / total) * 100,
			color: "green",
			tooltip: `Success: ${outcomeBreakdown.success.toLocaleString()}`,
		},
		{
			value: (outcomeBreakdown.failure / total) * 100,
			color: "red",
			tooltip: `Failure: ${outcomeBreakdown.failure.toLocaleString()}`,
		},
		{
			value: (outcomeBreakdown.partial / total) * 100,
			color: "yellow",
			tooltip: `Partial: ${outcomeBreakdown.partial.toLocaleString()}`,
		},
	].filter((s) => s.value > 0);

	return (
		<Paper className={classes.chartCard} p="md" withBorder>
			<Text size="sm" fw={600} mb="md">
				Outcome Distribution
			</Text>
			<Group justify="center" gap="xl">
				<RingProgress
					size={140}
					thickness={16}
					roundCaps
					sections={sections}
					label={
						<Center>
							<Stack gap={0} align="center">
								<Text size="lg" fw={700}>
									{total.toLocaleString()}
								</Text>
								<Text size="xs" c="dimmed">
									Total
								</Text>
							</Stack>
						</Center>
					}
				/>
				<Stack gap="xs">
					{(["success", "failure", "partial"] as const).map((outcome) => {
						const count = outcomeBreakdown[outcome] || 0;
						const percent = total > 0 ? ((count / total) * 100).toFixed(1) : "0";
						const Icon = outcome === "success" ? IconCheck : outcome === "failure" ? IconX : IconAlertTriangle;

						return (
							<Group key={outcome} gap="sm">
								<ThemeIcon size="xs" color={getOutcomeColor(outcome)} variant="filled">
									<Icon size={10} />
								</ThemeIcon>
								<Text size="sm" style={{ minWidth: 60 }}>
									{outcome.charAt(0).toUpperCase() + outcome.slice(1)}
								</Text>
								<Text size="sm" c="dimmed">
									{count.toLocaleString()} ({percent}%)
								</Text>
							</Group>
						);
					})}
				</Stack>
			</Group>
		</Paper>
	);
}

function ActionBreakdown({ actionBreakdown }: { actionBreakdown: Record<AuditAction, number> }) {
	const sorted = useMemo(() => {
		return Object.entries(actionBreakdown)
			.sort(([, a], [, b]) => b - a)
			.slice(0, 8);
	}, [actionBreakdown]);

	const max = Math.max(...sorted.map(([, count]) => count));

	if (sorted.length === 0) return null;

	return (
		<Paper className={classes.chartCard} p="md" withBorder>
			<Text size="sm" fw={600} mb="md">
				Actions by Type
			</Text>
			<Stack gap="sm">
				{sorted.map(([action, count]) => (
					<div key={action}>
						<Group justify="space-between" mb={4}>
							<Badge size="sm" variant="light" color={getActionColor(action as AuditAction)}>
								{getActionLabel(action as AuditAction)}
							</Badge>
							<Text size="xs" c="dimmed">
								{count.toLocaleString()}
							</Text>
						</Group>
						<Progress
							value={(count / max) * 100}
							size="sm"
							color={getActionColor(action as AuditAction)}
							radius="sm"
						/>
					</div>
				))}
			</Stack>
		</Paper>
	);
}

function TopUsers({ topUsers }: { topUsers: AuditAnalyticsType["topUsers"] }) {
	if (!topUsers || topUsers.length === 0) return null;

	const max = Math.max(...topUsers.map((u) => u.count));

	return (
		<Paper className={classes.chartCard} p="md" withBorder>
			<Text size="sm" fw={600} mb="md">
				Top Users by Activity
			</Text>
			<Stack gap="sm">
				{topUsers.slice(0, 5).map((user, index) => (
					<div key={user.userId}>
						<Group justify="space-between" mb={4}>
							<Group gap="xs">
								<Badge size="xs" variant="filled" color="gray" circle>
									{index + 1}
								</Badge>
								<Text size="sm">
									{user.userName || user.userId}
								</Text>
							</Group>
							<Text size="xs" c="dimmed">
								{user.count.toLocaleString()} events
							</Text>
						</Group>
						<Progress
							value={(user.count / max) * 100}
							size="sm"
							color="blue"
							radius="sm"
						/>
					</div>
				))}
			</Stack>
		</Paper>
	);
}

function TopResources({ topResources }: { topResources: AuditAnalyticsType["topResources"] }) {
	if (!topResources || topResources.length === 0) return null;

	const max = Math.max(...topResources.map((r) => r.count));

	return (
		<Paper className={classes.chartCard} p="md" withBorder>
			<Text size="sm" fw={600} mb="md">
				Most Accessed Resources
			</Text>
			<Stack gap="sm">
				{topResources.slice(0, 5).map((resource, index) => (
					<div key={`${resource.resourceType}-${resource.resourceId || index}`}>
						<Group justify="space-between" mb={4}>
							<Group gap="xs">
								<Badge size="xs" variant="filled" color="gray" circle>
									{index + 1}
								</Badge>
								<Text size="sm">
									{resource.resourceType}
									{resource.resourceId && `/${resource.resourceId.slice(0, 8)}...`}
								</Text>
							</Group>
							<Text size="xs" c="dimmed">
								{resource.count.toLocaleString()} events
							</Text>
						</Group>
						<Progress
							value={(resource.count / max) * 100}
							size="sm"
							color="violet"
							radius="sm"
						/>
					</div>
				))}
			</Stack>
		</Paper>
	);
}

function FailedAttempts({ failedAttempts }: { failedAttempts: AuditAnalyticsType["failedAttempts"] }) {
	if (!failedAttempts || failedAttempts.length === 0) return null;

	return (
		<Paper className={classes.chartCard} p="md" withBorder>
			<Text size="sm" fw={600} mb="md">
				Failed Action Attempts
			</Text>
			<Stack gap="sm">
				{failedAttempts.slice(0, 5).map((attempt) => (
					<Group key={attempt.action} justify="space-between">
						<Group gap="xs">
							<ThemeIcon size="sm" color="red" variant="light">
								<IconX size={12} />
							</ThemeIcon>
							<Text size="sm">{getActionLabel(attempt.action)}</Text>
						</Group>
						<Group gap="xs">
							<Badge size="sm" color="red" variant="light">
								{attempt.count} failures
							</Badge>
							<Text size="xs" c="dimmed">
								{new Date(attempt.lastAttempt).toLocaleDateString()}
							</Text>
						</Group>
					</Group>
				))}
			</Stack>
		</Paper>
	);
}

function ActivityTimeline({ activityOverTime }: { activityOverTime: AuditAnalyticsType["activityOverTime"] }) {
	if (!activityOverTime || activityOverTime.length === 0) return null;

	const max = Math.max(...activityOverTime.map((p) => p.count));

	return (
		<Paper className={classes.chartCard} p="md" withBorder>
			<Text size="sm" fw={600} mb="md">
				Activity Over Time
			</Text>
			<div className={classes.timeline}>
				{activityOverTime.slice(-24).map((point) => {
					const height = max > 0 ? (point.count / max) * 100 : 0;
					const date = new Date(point.timestamp);

					return (
						<div
							key={point.timestamp}
							className={classes.timelineBar}
							style={{ height: `${Math.max(height, 2)}%` }}
							title={`${date.toLocaleString()}: ${point.count} events`}
						/>
					);
				})}
			</div>
			<Group justify="space-between" mt="xs">
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
			</Group>
		</Paper>
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
				<Stack align="center" gap="sm">
					<ThemeIcon size={48} variant="light" color="gray" radius="xl">
						<IconTrendingUp size={24} />
					</ThemeIcon>
					<Text c="dimmed">No analytics data available</Text>
				</Stack>
			</Center>
		);
	}

	const totalEvents = Object.values(analytics.outcomeBreakdown).reduce((a, b) => a + b, 0);
	const successRate = totalEvents > 0
		? ((analytics.outcomeBreakdown.success / totalEvents) * 100).toFixed(1)
		: "0";

	return (
		<Stack gap="md" p="md">
			{/* Summary Stats */}
			<SimpleGrid cols={{ base: 1, sm: 2, md: 4 }} spacing="md">
				<StatCard
					title="Total Events"
					value={totalEvents}
					icon={IconClock}
					color="blue"
				/>
				<StatCard
					title="Success Rate"
					value={`${successRate}%`}
					icon={IconCheck}
					color="green"
				/>
				<StatCard
					title="Active Users"
					value={analytics.topUsers.length}
					icon={IconUser}
					color="violet"
				/>
				<StatCard
					title="Resources Accessed"
					value={analytics.topResources.length}
					icon={IconDatabase}
					color="orange"
				/>
			</SimpleGrid>

			{/* Activity Timeline */}
			<ActivityTimeline activityOverTime={analytics.activityOverTime} />

			{/* Charts Grid */}
			<SimpleGrid cols={{ base: 1, md: 2 }} spacing="md">
				<OutcomeRing outcomeBreakdown={analytics.outcomeBreakdown} />
				<ActionBreakdown actionBreakdown={analytics.actionBreakdown} />
				<TopUsers topUsers={analytics.topUsers} />
				<TopResources topResources={analytics.topResources} />
			</SimpleGrid>

			{/* Failed Attempts */}
			{analytics.failedAttempts.length > 0 && (
				<FailedAttempts failedAttempts={analytics.failedAttempts} />
			)}
		</Stack>
	);
}

export const AuditAnalytics = memo(AuditAnalyticsComponent);
