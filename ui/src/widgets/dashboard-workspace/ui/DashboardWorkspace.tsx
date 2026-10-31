import {
	Button,
	DataTable,
	type DataTableColumn,
	EmptyState,
	getResourceDisplayName,
	ReferenceLink,
} from "@octofhir/ui-kit";
import { useQueries, useQuery } from "@tanstack/react-query";
import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import { BookOpen, Boxes, DatabaseZap, FileText, Send, Terminal } from "lucide-react";
import { useMemo } from "react";
import { useLoadDemoData } from "@/entities/demo-data";
import { getFhirCatalogSummary } from "@/entities/fhir-catalog";
import { getHealthStatusView } from "@/entities/system-health";
import { fhirClient } from "@/shared/api";
import {
	useBuildInfo,
	useDbTables,
	useHealth,
	useResourceTypesCategorized,
	useSettings,
} from "@/shared/api/hooks";
import type { FhirResource, ServerFeatures } from "@/shared/api/types";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import classes from "./DashboardWorkspace.module.css";

dayjs.extend(relativeTime);

interface DashboardWorkspaceProps {
	onNavigate: (href: string) => void;
}

const FEATURES: Array<[keyof ServerFeatures, string]> = [
	["sqlOnFhir", "SQL on FHIR"],
	["graphql", "GraphQL"],
	["bulkExport", "Bulk export"],
	["dbConsole", "DB console"],
	["auth", "Auth"],
	["cql", "CQL"],
];

/** How many of the largest types feed the "recently updated" list. */
const RECENT_SOURCE_TYPES = 6;
/** Tables counted exactly for the "largest types" panel. */
const CANDIDATE_TYPES = 10;

interface TypeCount {
	type: string;
	rows: number;
}

/**
 * Largest user-data types. Table stats only pick the candidates (planner estimates are 0
 * until Postgres analyzes a fresh table); the numbers themselves come from `_summary=count`.
 */
function useLargestTypes(): { items: TypeCount[]; available: boolean; isLoading: boolean } {
	const { data: tables, isError, isLoading: tablesLoading } = useDbTables();
	const { data: catalog } = useResourceTypesCategorized();

	const candidates = useMemo(() => {
		if (!tables || !catalog) return [];
		const byTable = new Map(
			catalog.types
				.filter((type) => type.category !== "system")
				.map((type) => [type.name.toLowerCase(), type.name]),
		);
		return tables.tables
			.filter((table) => table.schema === "public" && byTable.has(table.name))
			.sort(
				(a, b) =>
					(b.rowEstimate ?? 0) - (a.rowEstimate ?? 0) ||
					(b.totalSizeBytes ?? 0) - (a.totalSizeBytes ?? 0),
			)
			.slice(0, CANDIDATE_TYPES)
			.flatMap((table) => byTable.get(table.name) ?? []);
	}, [tables, catalog]);

	const counts = useQueries({
		queries: candidates.map((type) => ({
			queryKey: ["dashboard", "count", type],
			queryFn: () => fhirClient.search(type, { _summary: "count" }),
			staleTime: 30_000,
		})),
	});

	const items = useMemo(
		() =>
			candidates
				.map((type, index) => ({ type, rows: counts[index]?.data?.total ?? 0 }))
				.filter((item) => item.rows > 0)
				.sort((a, b) => b.rows - a.rows),
		[candidates, counts],
	);

	return {
		items,
		available: !isError,
		isLoading: tablesLoading || counts.some((count) => count.isLoading),
	};
}

function useRecentlyUpdated(types: string[]) {
	const results = useQueries({
		queries: types.map((type) => ({
			queryKey: ["dashboard", "recent", type],
			queryFn: () => fhirClient.search(type, { _sort: "-_lastUpdated", _count: 5 }),
			staleTime: 30_000,
		})),
	});
	const isLoading = results.some((result) => result.isLoading);
	const resources = useMemo(
		() =>
			results
				.flatMap((result) => result.data?.entry?.map((entry) => entry.resource) ?? [])
				.filter((resource): resource is FhirResource => Boolean(resource?.id))
				.sort((a, b) => (b.meta?.lastUpdated ?? "").localeCompare(a.meta?.lastUpdated ?? ""))
				.slice(0, 10),
		[results],
	);
	return { resources, isLoading };
}

export function DashboardWorkspace({ onNavigate }: DashboardWorkspaceProps) {
	const { data: health, isLoading: isHealthLoading } = useHealth();
	const { data: buildInfo } = useBuildInfo();
	const { data: settings } = useSettings();
	const { data: catalog } = useResourceTypesCategorized();
	const largest = useLargestTypes();
	const recent = useRecentlyUpdated(
		largest.items.slice(0, RECENT_SOURCE_TYPES).map((item) => item.type),
	);

	// No patients is the clearest sign of a fresh server: offer sample data instead of empty tables.
	const patients = useQuery({
		queryKey: ["dashboard", "count", "Patient"],
		queryFn: () => fhirClient.search("Patient", { _summary: "count" }),
		staleTime: 30_000,
	});
	const isEmptyServer = patients.data?.total === 0;
	const loadDemoData = useLoadDemoData();

	const healthView = getHealthStatusView(health?.status);
	const catalogSummary = getFhirCatalogSummary(catalog);
	const maxRows = largest.items[0]?.rows ?? 1;

	const recentColumns: DataTableColumn<FhirResource>[] = [
		{
			id: "ref",
			header: "Resource",
			width: 300,
			cell: (resource) => (
				<ReferenceLink
					reference={`${resource.resourceType}/${resource.id}`}
					href={`${import.meta.env.BASE_URL}resources/${resource.resourceType}/${resource.id}`}
					onClick={(event) => {
						event.preventDefault();
						onNavigate(`/resources/${resource.resourceType}/${resource.id}`);
					}}
				/>
			),
		},
		{
			id: "name",
			header: "Name",
			cell: (resource) => {
				const name = getResourceDisplayName(resource, "");
				return name && name !== resource.id ? name : <span className={classes.muted}>—</span>;
			},
		},
		{
			id: "updated",
			header: "Updated",
			width: 120,
			cell: (resource) =>
				resource.meta?.lastUpdated ? (
					<span className={classes.muted} title={new Date(resource.meta.lastUpdated).toLocaleString()}>
						{dayjs(resource.meta.lastUpdated).fromNow()}
					</span>
				) : (
					<span className={classes.muted}>—</span>
				),
		},
	];

	return (
		<WorkspacePageLayout
			title="Overview"
			description="Server state, recent activity and where the data is."
			className="page-enter"
			actions={
				<>
					<Button variant="filled" onClick={() => onNavigate("/console")}>
						<Button.Icon side="left">
							<Send size={14} />
						</Button.Icon>
						New request
					</Button>
					<Button variant="outline" onClick={() => onNavigate("/db-console")}>
						<Button.Icon side="left">
							<Terminal size={14} />
						</Button.Icon>
						SQL
					</Button>
					<Button variant="outline" onClick={() => onNavigate("/packages")}>
						<Button.Icon side="left">
							<Boxes size={14} />
						</Button.Icon>
						Packages
					</Button>
				</>
			}
		>
			<section className={classes.status} aria-label="Server status">
				<div className={classes.cell}>
					<span className={classes.label}>Server</span>
					<span className={classes.value}>
						<span className={classes.dot} data-tone={healthView.tone} />
						{isHealthLoading ? "Checking…" : healthView.label}
					</span>
					<span className={classes.caption}>{health?.details ?? healthView.caption}</span>
				</div>
				<div className={classes.cell}>
					<span className={classes.label}>Version</span>
					<span className={classes.value}>{buildInfo?.serverVersion ?? "dev"}</span>
					<span className={classes.captionMono}>{buildInfo?.commit?.slice(0, 8) ?? "local build"}</span>
				</div>
				<div className={classes.cell}>
					<span className={classes.label}>FHIR</span>
					<span className={classes.value}>{settings?.fhirVersion ?? "—"}</span>
					<span className={classes.caption}>Active specification</span>
				</div>
				<div className={classes.cell}>
					<span className={classes.label}>Resource types</span>
					<span className={classes.value}>{catalogSummary.total}</span>
					<span className={classes.caption}>{catalogSummary.caption}</span>
				</div>
			</section>

			<section className={classes.features} aria-label="Capabilities">
				{FEATURES.map(([key, label]) => {
					const enabled = Boolean(settings?.features[key]);
					return (
						<span key={key} className={classes.feature} data-enabled={enabled}>
							<span className={classes.dot} data-tone={enabled ? "success" : "neutral"} />
							{label}
							<span className={classes.featureState}>{enabled ? "on" : "off"}</span>
						</span>
					);
				})}
			</section>

			{isEmptyServer && (
				<section className={classes.welcome} aria-label="Get started">
					<DatabaseZap size={20} className={classes.welcomeIcon} aria-hidden="true" />
					<div className={classes.welcomeText}>
						<h2 className={classes.welcomeTitle}>This server has no data yet</h2>
						<p className={classes.welcomeBody}>
							Load a small sample clinic (30 patients with encounters, vitals and conditions) to try
							search, the consoles and the resource browser. Everything is tagged, so{" "}
							<code className={classes.code}>?_tag=demo-data</code> finds it again.
						</p>
					</div>
					<div className={classes.welcomeActions}>
						<Button variant="filled" loading={loadDemoData.isPending} onClick={() => loadDemoData.mutate()}>
							Load demo data
						</Button>
						<Button
							variant="outline"
							onClick={() => window.open("https://octofhir.github.io/server-rs/getting-started/", "_blank")}
						>
							<Button.Icon side="left">
								<BookOpen size={14} />
							</Button.Icon>
							Docs
						</Button>
					</div>
				</section>
			)}

			<div className={classes.columns}>
				<section className={classes.panel}>
					<h2 className={classes.panelTitle}>Recently updated</h2>
					<DataTable
						aria-label="Recently updated resources"
						data={recent.resources}
						columns={recentColumns}
						getRowId={(resource) => `${resource.resourceType}/${resource.id}`}
						loading={recent.isLoading || largest.isLoading}
						loadingRowCount={6}
						onRowClick={(resource) => onNavigate(`/resources/${resource.resourceType}/${resource.id}`)}
						emptyState={
							<EmptyState
								image={<FileText width={32} height={32} aria-hidden="true" />}
								title="No resources yet"
								description="Create one with a request or load a package with examples."
							/>
						}
					/>
				</section>

				<section className={classes.panel}>
					<h2 className={classes.panelTitle}>Largest types</h2>
					{largest.items.length === 0 ? (
						<p className={classes.empty}>
							{largest.available ? "No stored resources yet." : "Table statistics need the DB console feature."}
						</p>
					) : (
						<ul className={classes.typeList}>
							{largest.items.slice(0, 10).map((item) => (
								<li key={item.type}>
									<button
										type="button"
										className={classes.typeRow}
										onClick={() => onNavigate(`/resources/${item.type}`)}
									>
										<span className={classes.typeName}>{item.type}</span>
										<span className={classes.bar} aria-hidden="true">
											<span style={{ width: `${Math.max(4, (item.rows / maxRows) * 100)}%` }} />
										</span>
										<span className={classes.typeCount}>{item.rows.toLocaleString()}</span>
									</button>
								</li>
							))}
						</ul>
					)}
				</section>
			</div>
		</WorkspacePageLayout>
	);
}
