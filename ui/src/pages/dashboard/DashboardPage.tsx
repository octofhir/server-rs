import { useNavigate } from "react-router-dom";
import {
	Stack,
	Title,
	Text,
	SimpleGrid,
	Card,
	Group,
	Badge,
	Button,
	Loader,
	ThemeIcon,
} from "@mantine/core";
import {
	IconFolder,
	IconTerminal,
	IconDatabase,
	IconActivity,
	IconFileDescription,
	IconSettings,
	IconCode,
	IconServer,
} from "@tabler/icons-react";
import { useHealth, useCapabilities } from "@/shared/api/hooks";

interface QuickAction {
	title: string;
	description: string;
	href: string;
	icon: typeof IconFolder;
	color: string;
}

const quickActions: QuickAction[] = [
	{
		title: "Browse Resources",
		description: "View and search FHIR resources",
		href: "/resources",
		icon: IconFolder,
		color: "blue",
	},
	{
		title: "REST Console",
		description: "Test FHIR API endpoints",
		href: "/console",
		icon: IconTerminal,
		color: "green",
	},
	{
		title: "DB Console",
		description: "Execute SQL queries",
		href: "/db-console",
		icon: IconDatabase,
		color: "orange",
	},
	{
		title: "GraphQL",
		description: "GraphQL query console",
		href: "/graphql",
		icon: IconCode,
		color: "grape",
	},
	{
		title: "API Gateway",
		description: "Manage custom endpoints",
		href: "/gateway",
		icon: IconServer,
		color: "cyan",
	},
	{
		title: "System Logs",
		description: "View server activity logs",
		href: "/logs",
		icon: IconActivity,
		color: "pink",
	},
	{
		title: "Capability Statement",
		description: "View server metadata",
		href: "/metadata",
		icon: IconFileDescription,
		color: "indigo",
	},
	{
		title: "Settings",
		description: "Configure server settings",
		href: "/settings",
		icon: IconSettings,
		color: "gray",
	},
];

function StatusCard() {
	const { data: health, isLoading } = useHealth();

	const statusColor = {
		ok: "green",
		degraded: "yellow",
		down: "red",
	}[health?.status ?? "down"];

	return (
		<Card shadow="sm" padding="lg" radius="md" withBorder>
			<Group justify="space-between" mb="xs">
				<Text fw={500}>Server Status</Text>
				<ThemeIcon variant="light" color={statusColor} size="lg">
					<IconActivity size={18} />
				</ThemeIcon>
			</Group>
			{isLoading ? (
				<Loader size="sm" />
			) : (
				<Badge color={statusColor} variant="light" size="lg">
					{health?.status ?? "Unknown"}
				</Badge>
			)}
		</Card>
	);
}

function ResourceTypesCard() {
	const { data: capabilities, isLoading } = useCapabilities();

	// Extract resource count from capability statement
	const resourceCount =
		(capabilities as any)?.rest?.[0]?.resource?.length ?? 0;

	return (
		<Card shadow="sm" padding="lg" radius="md" withBorder>
			<Group justify="space-between" mb="xs">
				<Text fw={500}>Resource Types</Text>
				<ThemeIcon variant="light" color="blue" size="lg">
					<IconFolder size={18} />
				</ThemeIcon>
			</Group>
			{isLoading ? (
				<Loader size="sm" />
			) : (
				<Group gap="xs">
					<Text size="xl" fw={700}>
						{resourceCount}
					</Text>
					<Text size="sm" c="dimmed">
						available types
					</Text>
				</Group>
			)}
		</Card>
	);
}

export function DashboardPage() {
	const navigate = useNavigate();

	return (
		<Stack gap="lg">
			<div>
				<Title order={2}>Dashboard</Title>
				<Text c="dimmed">Welcome to OctoFHIR Server Console</Text>
			</div>

			<SimpleGrid cols={{ base: 1, sm: 2 }}>
				<StatusCard />
				<ResourceTypesCard />
			</SimpleGrid>

			<div>
				<Title order={3} mb="md">
					Quick Actions
				</Title>
				<SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }}>
					{quickActions.map((action) => (
						<Card
							key={action.href}
							shadow="sm"
							padding="lg"
							radius="md"
							withBorder
						>
							<ThemeIcon
								variant="light"
								color={action.color}
								size="xl"
								mb="md"
							>
								<action.icon size={24} />
							</ThemeIcon>
							<Text fw={500} mb="xs">
								{action.title}
							</Text>
							<Text size="sm" c="dimmed" mb="md">
								{action.description}
							</Text>
							<Button
								variant="light"
								size="xs"
								onClick={() => navigate(action.href)}
							>
								Open
							</Button>
						</Card>
					))}
				</SimpleGrid>
			</div>
		</Stack>
	);
}

