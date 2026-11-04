import { useMemo, useState } from "react";
import {
	Alert,
	Autocomplete,
	Button,
	DataTable,
	type DataTableColumn,
	DropdownMenu,
	EmptyState,
	SegmentedRadioGroup,
	Select,
	StatGrid,
	StatusBadge,
	type StatusTone,
	Text,
	TextInput,
	notifications,
	useDebouncedValue,
} from "@octofhir/ui-kit";
import { ArrowRight, Download, GitCompareArrows, Search as Magnifier } from "lucide-react";
import { usePackageDiff, usePackageLookup, usePackageSearch, usePackages } from "@/shared/api/hooks";
import { serverApi } from "@/shared/api/serverApi";
import type {
	PackageDiffChange,
	PackageDiffDownloadFormat,
	PackageDiffProfile,
	PackageDiffReport,
	PackageDiffRequest,
	PackageDiffSeverity,
} from "@/shared/api/types";
import classes from "./PackageCompareTab.module.css";

type Threshold = "breaking" | "warning" | "info";

interface ChangeRow {
	id: string;
	profile: PackageDiffProfile;
	change: PackageDiffChange;
}

const RANK: Record<PackageDiffSeverity, number> = { info: 0, warning: 1, breaking: 2 };

const TONE: Record<PackageDiffSeverity, StatusTone> = {
	breaking: "danger",
	warning: "warning",
	info: "neutral",
};

const DOWNLOADS: { format: PackageDiffDownloadFormat; label: string; extension: string; type: string }[] = [
	{ format: "markdown", label: "Markdown report", extension: "md", type: "text/markdown" },
	{ format: "sarif", label: "SARIF 2.1.0", extension: "sarif", type: "application/sarif+json" },
	{ format: "text", label: "Plain text", extension: "txt", type: "text/plain" },
];

/** Versions newest first (`9.0.0`, `8.0.0`, `6.1.0`, `6.1.0-snapshot1`). */
function newestFirst(versions: string[]): string[] {
	return [...versions].sort((a, b) => b.localeCompare(a, undefined, { numeric: true }));
}

/** A package with two installed versions to start with; otherwise nothing is preselected. */
function defaultName(packages: { name: string }[]): string {
	const counts = new Map<string, number>();
	for (const p of packages) counts.set(p.name, (counts.get(p.name) ?? 0) + 1);
	return [...counts].find(([, n]) => n > 1)?.[0] ?? "";
}

function count(report: PackageDiffReport, severity: PackageDiffSeverity): number {
	return report.profiles.reduce(
		(sum, p) => sum + p.changes.filter((c) => c.severity === severity).length,
		0,
	);
}

function saveFile(text: string, name: string, type: string) {
	const url = URL.createObjectURL(new Blob([text], { type: `${type};charset=utf-8` }));
	const link = document.createElement("a");
	link.href = url;
	link.download = name;
	document.body.appendChild(link);
	link.click();
	link.remove();
	URL.revokeObjectURL(url);
}

const COLUMNS: DataTableColumn<ChangeRow>[] = [
	{
		id: "severity",
		header: "Severity",
		width: 104,
		accessor: (row) => RANK[row.change.severity],
		sortable: true,
		cell: (row) => <StatusBadge tone={TONE[row.change.severity]}>{row.change.severity}</StatusBadge>,
	},
	{
		id: "profile",
		header: "Profile",
		width: 260,
		accessor: (row) => row.profile.name,
		sortable: true,
		cell: (row) => (
			<span className={classes.profile} title={row.profile.url}>
				<span className={classes.truncate}>{row.profile.name}</span>
				<span className={classes.dim}>{row.profile.type_name}</span>
			</span>
		),
	},
	{
		id: "rule",
		header: "Rule",
		width: 190,
		accessor: (row) => row.change.rule,
		sortable: true,
		cell: (row) => <code className={classes.mono}>{row.change.rule}</code>,
	},
	{
		id: "path",
		header: "Element",
		width: 280,
		accessor: (row) => row.change.path,
		sortable: true,
		cell: (row) => (
			<code className={`${classes.mono} ${classes.truncate}`} title={row.change.path}>
				{row.change.path}
			</code>
		),
	},
	{
		id: "message",
		header: "Change",
		accessor: (row) => row.change.message,
		cell: (row) => (
			<span className={classes.truncate} title={row.change.message}>
				{row.change.message}
			</span>
		),
	},
];

export function PackageCompareTab() {
	const { data } = usePackages();
	const installed = useMemo(() => data?.packages ?? [], [data]);
	const installedNames = useMemo(() => [...new Set(installed.map((p) => p.name))].sort(), [installed]);
	const [nameInput, setNameInput] = useState<string | null>(null);
	const [chosenName, setChosenName] = useState<string | null>(null);
	const [oldVersion, setOldVersion] = useState<string | null>(null);
	const [newVersion, setNewVersion] = useState<string | null>(null);
	const [request, setRequest] = useState<PackageDiffRequest | null>(null);
	const [threshold, setThreshold] = useState<Threshold>("warning");
	const [search, setSearch] = useState("");
	const [downloading, setDownloading] = useState(false);

	const needle = (nameInput ?? "").trim();
	const query = useDebouncedValue(needle.length >= 2 ? needle : "", 300);
	const registrySearch = usePackageSearch(query);
	const found = useMemo(
		() => registrySearch.data?.packages.map((p) => p.name) ?? [],
		[registrySearch.data],
	);
	// A name typed in full is chosen without picking it from the list.
	const typedExactly = needle && (installedNames.includes(needle) || found.includes(needle)) ? needle : null;
	const name = typedExactly ?? chosenName ?? defaultName(installed);

	const lookup = usePackageLookup(name);
	const installedVersions = useMemo(
		() => new Set(installed.filter((p) => p.name === name).map((p) => p.version)),
		[installed, name],
	);
	// Published and installed versions: a guide installed from a build is in no registry.
	const versions = useMemo(
		() => newestFirst([...new Set([...(lookup.data?.versions ?? []), ...installedVersions])]),
		[lookup.data, installedVersions],
	);
	// By default: the newest installed version against the newest one.
	const latest = versions[0] ?? null;
	const installedOther = versions.find((v) => installedVersions.has(v) && v !== latest);
	const oldValue = oldVersion ?? installedOther ?? versions[1] ?? null;
	const newValue = newVersion ?? latest;

	const nameOptions = useMemo(() => {
		const lower = needle.toLowerCase();
		const names = [...new Set([...found, ...installedNames])].filter(
			(n) => !lower || n.toLowerCase().includes(lower),
		);
		names.sort((a, b) => Number(b === needle) - Number(a === needle));
		return names.slice(0, 30).map((value) => ({
			value,
			label: installedNames.includes(value) ? `${value} · installed` : value,
			textValue: value,
		}));
	}, [found, installedNames, needle]);
	const versionOptions = versions.map((value) => ({
		value,
		label: installedVersions.has(value) ? `${value} · installed` : value,
	}));

	const diff = usePackageDiff(request);
	const report = diff.data;
	const rows = useMemo<ChangeRow[]>(() => {
		if (!report) return [];
		const needle = search.trim().toLowerCase();
		const minimum = RANK[threshold];
		return report.profiles.flatMap((profile) =>
			profile.changes
				.filter((change) => RANK[change.severity] >= minimum)
				.filter(
					(change) =>
						!needle ||
						[profile.name, profile.url, change.rule, change.path, change.message].some((text) =>
							text.toLowerCase().includes(needle),
						),
				)
				.map((change, index) => ({ id: `${profile.url}#${index}#${change.rule}#${change.path}`, profile, change })),
		);
	}, [report, threshold, search]);

	const stats = useMemo(() => {
		if (!report) return [];
		const status = (s: PackageDiffProfile["status"]) => report.profiles.filter((p) => p.status === s).length;
		return [
			{ id: "profiles", label: "Profiles", value: report.profiles.length },
			{ id: "added", label: "Added", value: status("added") },
			{ id: "removed", label: "Removed", value: status("removed") },
			{ id: "changed", label: "Changed", value: status("changed") },
			{ id: "breaking", label: "Breaking", value: count(report, "breaking"), tone: "danger" as const },
			{ id: "warning", label: "Warnings", value: count(report, "warning"), tone: "warning" as const },
			{ id: "info", label: "Info", value: count(report, "info") },
		];
	}, [report]);

	const compare = () => {
		if (name && oldValue && newValue) {
			setRequest({ old: `${name}@${oldValue}`, new: `${name}@${newValue}` });
		}
	};

	const pickName = (value: string) => {
		setChosenName(value);
		setNameInput(value);
		setOldVersion(null);
		setNewVersion(null);
	};

	const download = async (format: PackageDiffDownloadFormat) => {
		if (!request) return;
		const target = DOWNLOADS.find((d) => d.format === format);
		if (!target) return;
		setDownloading(true);
		try {
			const text = await serverApi.downloadPackageDiff(request, format);
			saveFile(text, `fhir-diff-${request.old}-to-${request.new}.${target.extension}`, target.type);
		} catch (error) {
			notifications.show({
				title: "Export failed",
				message: error instanceof Error ? error.message : String(error),
				color: "red",
			});
		} finally {
			setDownloading(false);
		}
	};

	const sameChoice = oldValue !== null && oldValue === newValue;
	const downloadsNeeded = [oldValue, newValue].filter((v) => v && !installedVersions.has(v));

	return (
		<div className={classes.stack}>
			<div className={classes.toolbar}>
				<Autocomplete
					aria-label="Package"
					placeholder="Package, e.g. hl7.fhir.us.core"
					value={nameInput ?? name}
					onChange={(value) => {
						setNameInput(value);
						setOldVersion(null);
						setNewVersion(null);
					}}
					onPick={pickName}
					data={nameOptions}
					filter="server"
					loading={registrySearch.isFetching}
					className={classes.name}
				/>
				<Select
					aria-label="Old version"
					placeholder={lookup.isFetching ? "Loading…" : "Old version"}
					data={versionOptions}
					value={oldValue}
					onChange={setOldVersion}
					searchable
					className={classes.select}
				/>
				<ArrowRight size={16} className={classes.arrow} aria-hidden />
				<Select
					aria-label="New version"
					placeholder={lookup.isFetching ? "Loading…" : "New version"}
					data={versionOptions}
					value={newValue}
					onChange={setNewVersion}
					searchable
					className={classes.select}
				/>
				<Button
					leftSection={<GitCompareArrows size={14} />}
					onClick={compare}
					disabled={!name || !oldValue || !newValue || sameChoice}
					loading={diff.isFetching}
				>
					Compare
				</Button>
				{report && (
					<DropdownMenu
						items={DOWNLOADS.map((d) => ({ text: d.label, action: () => void download(d.format) }))}
						switcher={
							<Button variant="outline" leftSection={<Download size={14} />} loading={downloading}>
								Export
							</Button>
						}
					/>
				)}
			</div>
			<Text size="sm" c="dimmed">
				{downloadsNeeded.length > 0
					? `${downloadsNeeded.join(" and ")} ${downloadsNeeded.length > 1 ? "are" : "is"} not installed: the server reads ${downloadsNeeded.length > 1 ? "them" : "it"} from the package registry for this comparison and installs nothing.`
					: "Every profile and extension is merged over its base chain and compared element by element."}
			</Text>
			{lookup.error && name && versions.length === 0 && (
				<Alert color="warm" variant="light">
					{`${name} was not found in the package registry.`}
				</Alert>
			)}

			{diff.error && (
				<Alert color="fire" variant="light">
					{diff.error instanceof Error ? diff.error.message : "The comparison failed"}
				</Alert>
			)}

			{!report && !diff.isFetching && !diff.error && (
				<EmptyState
					image={<GitCompareArrows size={22} />}
					title="Compare two package versions"
					description="Choose a package and two of its versions, installed or not, then Compare: breaking changes, warnings and additions for every profile and extension."
				/>
			)}

			{report && (
				<>
					<StatGrid items={stats} />
					{report.notes && report.notes.length > 0 && (
						<Alert color="blue" variant="light" title="Notes">
							<ul className={classes.notes}>
								{report.notes.map((note) => (
									<li key={note}>{note}</li>
								))}
							</ul>
						</Alert>
					)}
					<div className={classes.filters}>
						<SegmentedRadioGroup
							aria-label="Minimum severity"
							value={threshold}
							onChange={(value) => setThreshold(value as Threshold)}
							options={[
								{ value: "breaking", label: "Breaking" },
								{ value: "warning", label: "Warnings and up" },
								{ value: "info", label: "All changes" },
							]}
						/>
						<TextInput
							placeholder="Filter by profile, element or rule"
							leftSection={<Magnifier size={14} />}
							value={search}
							onChange={(value) => setSearch(value)}
							className={classes.search}
						/>
						<Text size="sm" c="dimmed">
							{rows.length} changes
						</Text>
					</div>
					<DataTable
						aria-label="Package changes"
						data={rows}
						columns={COLUMNS}
						getRowId={(row) => row.id}
						virtualized
						maxHeight={560}
						stickyHeader
						emptyState={search ? `No changes match “${search}”.` : "No changes at this severity."}
					/>
				</>
			)}
		</div>
	);
}
