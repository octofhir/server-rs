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
	Box,
} from "@/shared/ui";
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
import { useHealth, useResourceTypes } from "@/shared/api/hooks";

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
		color: "primary",
	},
	{
		title: "REST Console",
		description: "Test FHIR API endpoints",
		href: "/console",
		icon: IconTerminal,
		color: "deep",
	},
	{
		title: "DB Console",
		description: "Execute SQL queries",
		href: "/db-console",
		icon: IconDatabase,
		color: "warm",
	},
	{
		title: "GraphQL",
		description: "GraphQL query console",
		href: "/graphql",
		icon: IconCode,
		color: "primary",
	},
	{
		title: "API Gateway",
		description: "Manage custom endpoints",
		href: "/gateway",
		icon: IconServer,
		color: "fire",
	},
	{
		title: "System Logs",
		description: "View server activity logs",
		href: "/logs",
		icon: IconActivity,
		color: "warm",
	},
	{
		title: "Capability Statement",
		description: "View server metadata",
		href: "/metadata",
		icon: IconFileDescription,
		color: "deep",
	},
	{
		title: "Settings",
		description: "Configure server settings",
		href: "/settings",
		icon: IconSettings,
		color: "deep",
	},
];

export function DashboardPage() {
	const navigate = useNavigate();

	return (
		<Box p="xl" className="page-enter">
			<Stack gap="xl">
				<Box>
					<Title order={1} style={{ letterSpacing: "-0.03em", fontWeight: 700 }}>
						Dashboard
					</Title>
					<Text c="dimmed" size="lg">
						Welcome to OctoFHIR Server Console
					</Text>
				</Box>

				<SimpleGrid cols={{ base: 1, sm: 2 }} spacing="lg">
					<StatusCard />
					<ResourceTypesCard />
				</SimpleGrid>

				<Box>
					<Group justify="space-between" mb="lg">
						<Title order={3} style={{ letterSpacing: "-0.02em" }}>
							Quick Actions
						</Title>
						<Badge variant="light" size="lg" radius="sm">
							Frequently Used
						</Badge>
					</Group>
					<SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }} spacing="lg">
						{quickActions.map((action) => (
							<Card
								key={action.href}
								withBorder
								padding="xl"
								radius="lg"
								className="dashboard-action-card"
								style={{
									backgroundColor: "var(--app-surface-1)",
									cursor: "pointer",
									transition: "all 0.2s ease",
								}}
								onClick={() => navigate(action.href)}
							>
								<ThemeIcon
									variant="light"
									color={action.color}
									size={52}
									radius="md"
									mb="lg"
									style={{
										boxShadow: `0 8px 16px var(--mantine-color-${action.color}-light-hover)`,
									}}
								>
									<action.icon size={28} />
								</ThemeIcon>
								<Text fw={600} size="lg" mb="xs" style={{ letterSpacing: "-0.01em" }}>
									{action.title}
								</Text>
								<Text size="sm" c="dimmed" mb="md" style={{ lineHeight: 1.5 }}>
									{action.description}
								</Text>
								<Button
									variant="subtle"
									color={action.color}
									size="sm"
									rightSection={<IconActivity size={14} />}
									p={0}
									style={{ width: "fit-content" }}
								>
									Open Tool
								</Button>
							</Card>
						))}
					</SimpleGrid>
				</Box>
			</Stack>

			<style dangerouslySetInnerHTML={{
				__html: `
				.dashboard-action-card:hover {
					transform: translateY(-4px);
					box-shadow: var(--mantine-shadow-md);
					border-color: var(--app-accent-primary);
				}
			`}} />
		</Box>
	);
}

function StatusCard() {
	const { data: health, isLoading } = useHealth();

	const statusColor = {
		ok: "primary",
		degraded: "warm",
		down: "fire",
	}[health?.status ?? "down"];

	return (
		<Card
			withBorder
			padding="xl"
			radius="lg"
			style={{
				backgroundColor: "var(--app-surface-1)",
				position: "relative",
				overflow: "hidden"
			}}
		>
			<Box
				style={{
					position: "absolute",
					top: 0,
					left: 0,
					right: 0,
					height: 4,
					background: `var(--mantine-color-${statusColor}-filled)`
				}}
			/>
			<Group justify="space-between" align="flex-start" mb="md">
				<div>
					<Text size="sm" fw={600} c="dimmed" tt="uppercase" style={{ letterSpacing: "0.05em" }} mb={4}>
						Server Status
					</Text>
					{isLoading ? (
						<Loader size="sm" variant="dots" />
					) : (
						<Title order={2} style={{ color: `var(--mantine-color-${statusColor}-filled)` }}>
							{health?.status?.toUpperCase() ?? "UNKNOWN"}
						</Title>
					)}
				</div>
				<ThemeIcon variant="light" color={statusColor} size={48} radius="md">
					<IconServer size={24} />
				</ThemeIcon>
			</Group>
			<Text size="xs" c="dimmed">
				Last check: {new Date().toLocaleTimeString()}
			</Text>
		</Card>
	);
}

function ResourceTypesCard() {
	const { data: resourceTypes = [], isLoading } = useResourceTypes();
	const resourceCount = resourceTypes.length;

	return (
		<Card
			withBorder
			padding="xl"
			radius="lg"
			style={{
				backgroundColor: "var(--app-surface-1)",
				position: "relative",
				overflow: "hidden"
			}}
		>
			<Box
				style={{
					position: "absolute",
					top: 0,
					left: 0,
					right: 0,
					height: 4,
					background: "var(--app-brand-gradient)"
				}}
			/>
			<Group justify="space-between" align="flex-start" mb="md">
				<div>
					<Text size="sm" fw={600} c="dimmed" tt="uppercase" style={{ letterSpacing: "0.05em" }} mb={4}>
						FHIR Resources
					</Text>
					{isLoading ? (
						<Loader size="sm" variant="dots" />
					) : (
						<Title order={2} style={{ letterSpacing: "-0.02em" }}>
							{resourceCount} Types
						</Title>
					)}
				</div>
				<ThemeIcon variant="light" color="primary" size={48} radius="md">
					<IconDatabase size={24} />
				</ThemeIcon>
			</Group>
			<Text size="xs" c="dimmed">
				Available in current schema
			</Text>
		</Card>
	);
}
