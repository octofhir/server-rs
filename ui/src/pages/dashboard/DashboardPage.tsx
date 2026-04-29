import { Badge, Box, Flex, Text } from "@/shared/ui";
import { useNavigate } from "react-router-dom";

import { MetricCard, ActionCard } from "@octofhir/ui-kit";
import {
	Folder,
	Terminal,
	Database,
	Pulse,
	FileText,
	Gear,
	Code,
	Server,
} from "@gravity-ui/icons";
import { useHealth, useResourceTypes } from "@/shared/api/hooks";

interface QuickAction {
	title: string;
	description: string;
	href: string;
	icon: any;
	color: string;
}

const quickActions: QuickAction[] = [
	{
		title: "Browse Resources",
		description: "View and search FHIR resources",
		href: "/resources",
		icon: Folder,
		color: "primary",
	},
	{
		title: "REST Console",
		description: "Test FHIR API endpoints",
		href: "/console",
		icon: Terminal,
		color: "deep",
	},
	{
		title: "DB Console",
		description: "Execute SQL queries",
		href: "/db-console",
		icon: Database,
		color: "warm",
	},
	{
		title: "GraphQL",
		description: "GraphQL query console",
		href: "/graphql",
		icon: Code,
		color: "primary",
	},
	{
		title: "API Gateway",
		description: "Manage custom endpoints",
		href: "/gateway",
		icon: Server,
		color: "fire",
	},
	{
		title: "System Logs",
		description: "View server activity logs",
		href: "/logs",
		icon: Pulse,
		color: "warm",
	},
	{
		title: "Capability Statement",
		description: "View server metadata",
		href: "/metadata",
		icon: FileText,
		color: "deep",
	},
	{
		title: "Settings",
		description: "Configure server settings",
		href: "/settings",
		icon: Gear,
		color: "deep",
	},
];

export function DashboardPage() {
	const navigate = useNavigate();

	return (
		<Box p="6" className="page-enter" style={{ height: "100%", overflow: "auto" }}>
			<Flex direction="column" gap="8" style={{ maxWidth: 1200, margin: "0 auto" }}>
				{/* Welcome Header */}
				<Box>
					<Text variant="header-2" style={{ letterSpacing: "-0.03em", fontWeight: 700 }}>
						System Dashboard
					</Text>
					<Text color="secondary" variant="body-2">
						Welcome to Abyxon FHIR Server Console. Monitor system health and access administrative tools.
					</Text>
				</Box>

				{/* Status Cards - Horizontal Row for KPI metrics */}
				<Flex gap="6" wrap="wrap">
					<Box style={{ flex: "1 1 300px" }}>
						<StatusCard />
					</Box>
					<Box style={{ flex: "1 1 300px" }}>
						<ResourceTypesCard />
					</Box>
				</Flex>

				{/* Quick Actions - Vertical list of functional blocks */}
				<Box>
					<Flex justifyContent="space-between" alignItems="center" mb="4">
						<Text variant="header-2" style={{ letterSpacing: "-0.02em" }}>
							Administrative Tools
						</Text>
						<Badge color="info" size="m">
							8 Active Modules
						</Badge>
					</Flex>
					
					<Box
						style={{
							display: "grid",
							gridTemplateColumns: "repeat(auto-fill, minmax(320px, 1fr))",
							gap: "20px",
						}}
					>
						{quickActions.map((action) => (
							<ActionCard
								key={action.href}
								title={action.title}
								description={action.description}
								icon={action.icon}
								color={action.color as any}
								onClick={() => navigate(action.href)}
							/>
						))}
					</Box>
				</Box>
			</Flex>
		</Box>
	);
}

function StatusCard() {
	const { data: health, isLoading } = useHealth();

	const statusColor = {
		ok: "success",
		degraded: "warning",
		down: "danger",
	}[health?.status ?? "down"];

	return (
		<MetricCard
			title="Server Status"
			value={health?.status?.toUpperCase() ?? "UNKNOWN"}
			isLoading={isLoading}
			icon={Server}
			color={statusColor as any}
			description={`Live health check active`}
		/>
	);
}

function ResourceTypesCard() {
	const { data: resourceTypes = [], isLoading } = useResourceTypes();
	const resourceCount = resourceTypes.length;

	return (
		<MetricCard
			title="FHIR Schema"
			value={`${resourceCount} Profiles`}
			isLoading={isLoading}
			icon={Database}
			color="info"
			description="Canonical resource definitions"
		/>
	);
}
