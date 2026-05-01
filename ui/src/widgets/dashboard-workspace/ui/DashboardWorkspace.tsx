import { Database, Gear, Play, Server, SquareListUl } from "@gravity-ui/icons";
import {
	FhirDashboardAside,
	WorkspaceBoard,
	type WorkspaceBoardColumn,
	type WorkspaceBoardMetric,
	type StatusTone,
} from "@octofhir/ui-kit";
import {
	useBuildInfo,
	useHealth,
	useResourceTypesCategorized,
	useSettings,
} from "@/shared/api/hooks";
import {
	consoleModuleLaneLabels,
	consoleModuleStatusLabels,
	consoleModules,
	type ConsoleModuleLane,
	type ConsoleModuleStatus,
} from "@/entities/console-module";
import { getFhirCatalogSummary } from "@/entities/fhir-catalog";
import { getHealthStatusView } from "@/entities/system-health";
import classes from "./DashboardWorkspace.module.css";

interface DashboardWorkspaceProps {
	onNavigate: (href: string) => void;
}

const statusTone: Record<ConsoleModuleStatus, StatusTone> = {
	ready: "success",
	watch: "warning",
	draft: "info",
};

const featureLabels: Array<[keyof NonNullable<ReturnType<typeof useSettings>["data"]>["features"], string]> = [
	["sqlOnFhir", "SQL"],
	["graphql", "GraphQL"],
	["bulkExport", "Bulk"],
	["dbConsole", "DB"],
	["auth", "Auth"],
	["cql", "CQL"],
];

const uiKitGoals = [
	{ id: "resource-primitives", label: "FHIR-aware resource primitives" },
	{ id: "questionnaire", label: "Questionnaire and form layer" },
	{ id: "storybook", label: "Storybook as UI contract" },
];

export function DashboardWorkspace({ onNavigate }: DashboardWorkspaceProps) {
	const { data: health, isLoading: isHealthLoading } = useHealth();
	const { data: buildInfo } = useBuildInfo();
	const { data: settings } = useSettings();
	const { data: catalog } = useResourceTypesCategorized();

	const healthView = getHealthStatusView(health?.status);
	const catalogSummary = getFhirCatalogSummary(catalog);
	const enabledFeatures = featureLabels.filter(([key]) => settings?.features[key]).length;

	const metrics: WorkspaceBoardMetric[] = [
		{
			id: "health",
			title: "Server status",
			value: isHealthLoading ? "Checking" : healthView.label,
			caption: health?.details ?? healthView.caption,
			icon: <Server size={18} />,
		},
		{
			id: "catalog",
			title: "Resource catalog",
			value: catalogSummary.total || "0",
			caption: catalogSummary.caption,
			icon: <Database size={18} />,
		},
		{
			id: "features",
			title: "FHIR capabilities",
			value: `${enabledFeatures}/${featureLabels.length}`,
			caption: settings?.fhirVersion ? `FHIR ${settings.fhirVersion}` : "Feature flags",
			icon: <SquareListUl size={18} />,
		},
		{
			id: "build",
			title: "Build",
			value: buildInfo?.serverVersion ?? "Local",
			caption: buildInfo?.commit ? buildInfo.commit.slice(0, 8) : "Development build",
			icon: <Gear size={18} />,
		},
	];

	const columns: WorkspaceBoardColumn[] = (["operate", "build", "govern"] as ConsoleModuleLane[]).map(
		(lane) => {
			const laneModules = consoleModules.filter((module) => module.lane === lane);

			return {
				id: lane,
				title: consoleModuleLaneLabels[lane],
				caption: lane === "operate" ? "Runtime workbench" : lane === "build" ? "FHIR builders" : "Access and audit",
				items: laneModules.map((module) => ({
					id: module.id,
					title: module.title,
					description: module.description,
					icon: module.icon,
					status: consoleModuleStatusLabels[module.status],
					statusTone: statusTone[module.status],
					meta: module.tags.map((tag) => ({
						id: tag,
						label: tag,
						tone: "neutral",
					})),
					onClick: () => onNavigate(module.href),
				})),
			};
		},
	);

	return (
		<div className={classes.shell}>
			<WorkspaceBoard
				eyebrow="OctoFHIR workspace"
				title="FHIR Control Plane"
				description="A command workspace for day-to-day server operations, FHIR modeling, and governance."
				actions={[
					{
						id: "rest",
						label: "REST console",
						icon: <Play size={16} />,
						view: "action",
						onClick: () => onNavigate("/console"),
					},
					{
						id: "settings",
						label: "Settings",
						icon: <Gear size={16} />,
						onClick: () => onNavigate("/settings"),
					},
				]}
				metrics={metrics}
				columns={columns}
				aside={
					<FhirDashboardAside
						surface={{
							fhirCount: catalogSummary.fhir,
							systemCount: catalogSummary.system,
							customCount: catalogSummary.custom,
							healthLabel: healthView.label,
							healthTone: healthView.tone,
						}}
						capabilities={featureLabels.map(([key, label]) => ({
							id: key,
							label,
							enabled: settings?.features[key],
						}))}
						checklist={{
							title: "FHIR UI goals",
							items: uiKitGoals,
						}}
					/>
				}
			/>
		</div>
	);
}
