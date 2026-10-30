import {
	Button,
	DataTable,
	type DataTableColumn,
	Loader,
	Tabs,
	Text,
	TextInput,
	useClipboard,
	WorkspacePageLayout,
} from "@octofhir/ui-kit";
import { Check, Copy, Search } from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useCapabilities } from "@/shared/api/hooks";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import classes from "./CapabilityStatementPage.module.css";

/** Interaction columns of the matrix, in REST order. */
const INTERACTIONS: Array<{ code: string; label: string }> = [
	{ code: "read", label: "read" },
	{ code: "vread", label: "vread" },
	{ code: "search-type", label: "search" },
	{ code: "create", label: "create" },
	{ code: "update", label: "update" },
	{ code: "patch", label: "patch" },
	{ code: "delete", label: "delete" },
	{ code: "history-instance", label: "hist·id" },
	{ code: "history-type", label: "hist·type" },
];

interface ResourceRow {
	type: string;
	interactions: Set<string>;
	searchParams: number;
}

type View = "overview" | "json";

export function CapabilityStatementPage() {
	const navigate = useNavigate();
	const { data, isLoading, error } = useCapabilities();
	const clipboard = useClipboard();
	const [view, setView] = useState<View>("overview");
	const [filter, setFilter] = useState("");

	const rest = data?.rest ?? [];
	const json = useMemo(() => (data ? JSON.stringify(data, null, 2) : ""), [data]);
	const systemInteractions = rest.flatMap((mode) => mode.interaction ?? []).flatMap((i) => i.code ?? []);

	const rows = useMemo<ResourceRow[]>(
		() =>
			rest
				.flatMap((mode) => mode.resource ?? [])
				.flatMap((resource) =>
					resource.type
						? [
								{
									type: resource.type,
									interactions: new Set(resource.interaction?.flatMap((i) => i.code ?? []) ?? []),
									searchParams: resource.searchParam?.length ?? 0,
								},
							]
						: [],
				),
		[rest],
	);
	const visibleRows = useMemo(() => {
		const query = filter.trim().toLowerCase();
		return query ? rows.filter((row) => row.type.toLowerCase().includes(query)) : rows;
	}, [rows, filter]);

	const columns: DataTableColumn<ResourceRow>[] = [
		{
			id: "type",
			header: "Resource",
			accessor: (row) => row.type,
			sortable: true,
			cell: (row) => <span className={classes.typeName}>{row.type}</span>,
		},
		...INTERACTIONS.map(
			({ code, label }): DataTableColumn<ResourceRow> => ({
				id: code,
				header: label,
				width: 72,
				align: "center",
				accessor: (row) => row.interactions.has(code),
				cell: (row) =>
					row.interactions.has(code) ? (
						<span className={classes.yes} role="img" aria-label="supported" />
					) : (
						<span className={classes.no} role="img" aria-label="not supported" />
					),
			}),
		),
		{
			id: "searchParams",
			header: "params",
			width: 80,
			align: "right",
			sortable: true,
			accessor: (row) => row.searchParams,
			cell: (row) => <span className={classes.count}>{row.searchParams}</span>,
		},
	];

	const meta = data
		? [data.fhirVersion && `FHIR ${data.fhirVersion}`, data.status, data.date?.slice(0, 10)]
				.filter(Boolean)
				.join(" · ")
		: undefined;

	return (
		<WorkspacePageLayout
			title="Capabilities"
			description={meta}
			className="page-enter"
			actions={
				data ? (
					<Button variant="outline" onClick={() => clipboard.copy(json)}>
						<Button.Icon side="left">
							{clipboard.copied ? <Check size={14} /> : <Copy size={14} />}
						</Button.Icon>
						{clipboard.copied ? "Copied" : "Copy JSON"}
					</Button>
				) : null
			}
			toolbar={
				<Tabs value={view} onChange={(next) => setView(next as View)}>
					<Tabs.List>
						<Tabs.Tab value="overview">Overview</Tabs.Tab>
						<Tabs.Tab value="json">JSON</Tabs.Tab>
					</Tabs.List>
				</Tabs>
			}
		>
			{isLoading ? (
				<div className={classes.loadingState}>
					<Loader size="sm" />
					<Text color="secondary">Loading capability statement…</Text>
				</div>
			) : error ? (
				<Text color="danger">
					{error instanceof Error ? error.message : "Failed to load capability statement"}
				</Text>
			) : !data ? null : view === "json" ? (
				<div className={classes.json}>
					<JsonEditor value={json} readOnly height="100%" />
				</div>
			) : (
				<>
					<section className={classes.server} aria-label="Server">
						<Field label="Software">
							{data.software?.name ?? "—"} <span className={classes.mono}>{data.software?.version}</span>
						</Field>
						<Field label="Endpoint">
							<span className={classes.mono}>{data.implementation?.url ?? "—"}</span>
						</Field>
						<Field label="Formats">
							<span className={classes.mono}>{data.format?.join("  ") || "—"}</span>
						</Field>
						<Field label="System interactions">
							<span className={classes.mono}>{systemInteractions.join("  ") || "—"}</span>
						</Field>
					</section>

					<section className={classes.matrix}>
						<div className={classes.matrixToolbar}>
							<TextInput
								aria-label="Filter resources"
								placeholder="Filter resources…"
								leftSection={<Search size={14} aria-hidden="true" />}
								value={filter}
								onChange={setFilter}
								className={classes.filter}
							/>
							<span className={classes.total}>
								{visibleRows.length} of {rows.length} resources
							</span>
						</div>
						<DataTable
							aria-label="Resource interactions"
							data={visibleRows}
							columns={columns}
							getRowId={(row) => row.type}
							stickyHeader
							maxHeight="calc(100vh - 330px)"
							onRowClick={(row) => navigate(`/resources/${row.type}`)}
						/>
					</section>
				</>
			)}
		</WorkspacePageLayout>
	);
}

function Field({ label, children }: { label: string; children: ReactNode }) {
	return (
		<div className={classes.field}>
			<span className={classes.label}>{label}</span>
			<span className={classes.value}>{children}</span>
		</div>
	);
}
