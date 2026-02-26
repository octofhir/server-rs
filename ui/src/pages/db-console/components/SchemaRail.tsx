import { useEffect, useMemo, useState } from "react";
import {
	ActionIcon,
	Box,
	Group,
	Loader,
	ScrollArea,
	SegmentedControl,
	Select,
	Text,
	TextInput,
	Tooltip,
	UnstyledButton,
} from "@/shared/ui";
import {
	IconDatabase,
	IconLayoutSidebarLeftCollapse,
	IconLayoutSidebarLeftExpand,
	IconSearch,
} from "@tabler/icons-react";
import { useDbTables } from "@/shared/api/hooks";
import type { DbTableInfo } from "@/shared/api/types";
import { TableDetailView } from "./TableDetailView";
import classes from "../DbConsolePage.module.css";

interface SchemaRailProps {
	expanded: boolean;
	onToggle: () => void;
	onInsertQuery: (query: string) => void;
	searchFocusKey?: number;
}

interface SelectedTable {
	schema: string;
	name: string;
}

type RailViewMode = "all" | "fhir" | "system";
type RailSortMode = "name" | "rows";

function isSystemTable(name: string): boolean {
	return name.startsWith("_");
}

function getTableAbbrev(name: string): string {
	const clean = name.startsWith("_") ? name.slice(1) : name;
	return clean.slice(0, 2);
}

function formatTableName(schema: string, name: string): string {
	return schema !== "public" ? `${schema}.${name}` : name;
}

export function SchemaRail({
	expanded,
	onToggle,
	onInsertQuery,
	searchFocusKey,
}: SchemaRailProps) {
	const { data, isLoading } = useDbTables();
	const [search, setSearch] = useState("");
	const [selected, setSelected] = useState<SelectedTable | null>(null);
	const [viewMode, setViewMode] = useState<RailViewMode>("all");
	const [sortMode, setSortMode] = useState<RailSortMode>("rows");

	// Clear selection when rail collapses
	useEffect(() => {
		if (!expanded) setSelected(null);
	}, [expanded]);

	const tables = data?.tables ?? [];

	const { fhirTables, systemTables } = useMemo(() => {
		const fhir: typeof tables = [];
		const system: typeof tables = [];
		for (const t of tables) {
			if (isSystemTable(t.name)) {
				system.push(t);
			} else {
				fhir.push(t);
			}
		}
		return { fhirTables: fhir, systemTables: system };
	}, [tables]);

	const filtered = useMemo(() => {
		if (!search.trim()) {
			return { all: tables, fhir: fhirTables, system: systemTables };
		}
		const q = search.toLowerCase();
		return {
			all: tables.filter(
				(t) =>
					t.name.toLowerCase().includes(q) ||
					t.schema.toLowerCase().includes(q),
			),
			fhir: fhirTables.filter(
				(t) =>
					t.name.toLowerCase().includes(q) ||
					t.schema.toLowerCase().includes(q),
			),
			system: systemTables.filter(
				(t) =>
					t.name.toLowerCase().includes(q) ||
					t.schema.toLowerCase().includes(q),
			),
		};
	}, [tables, fhirTables, systemTables, search]);

	const activeTables = useMemo(() => {
		switch (viewMode) {
			case "fhir":
				return filtered.fhir;
			case "system":
				return filtered.system;
			default:
				return filtered.all;
		}
	}, [filtered, viewMode]);

	const sortedActiveTables = useMemo(() => {
		const sorted = [...activeTables];
		sorted.sort((a, b) => {
			if (sortMode === "rows") {
				const aRows = a.rowEstimate ?? -1;
				const bRows = b.rowEstimate ?? -1;
				if (aRows !== bRows) {
					return bRows - aRows;
				}
			}

			const bySchema = a.schema.localeCompare(b.schema);
			if (bySchema !== 0) {
				return bySchema;
			}
			return a.name.localeCompare(b.name);
		});
		return sorted;
	}, [activeTables, sortMode]);

	const groupedTables = useMemo(() => {
		const groups = new Map<string, DbTableInfo[]>();
		for (const table of sortedActiveTables) {
			const schemaTables = groups.get(table.schema);
			if (schemaTables) {
				schemaTables.push(table);
			} else {
				groups.set(table.schema, [table]);
			}
		}

		return Array.from(groups.entries()).map(([schema, items]) => ({
			schema,
			items,
		}));
	}, [sortedActiveTables]);

	const handleTableClick = (schema: string, name: string) => {
		if (expanded) {
			setSelected({ schema, name });
		} else {
			onToggle();
		}
	};

	const handleInsertSelect = (schema: string, name: string) => {
		const prefix = schema !== "public" ? `${schema}.` : "";
		onInsertQuery(`SELECT * FROM ${prefix}${name} LIMIT 100;`);
	};

	const renderExpandedTable = (table: DbTableInfo) => (
		<UnstyledButton
			key={`${table.schema}.${table.name}`}
			className={`${classes.railItem} ${classes.railItemExpanded}`}
			onClick={() => handleTableClick(table.schema, table.name)}
			onDoubleClick={() => handleInsertSelect(table.schema, table.name)}
		>
			<div
				className={`${classes.railBadge} ${
					isSystemTable(table.name) ? classes.railBadgeSystem : classes.railBadgeFhir
				}`}
			>
				{getTableAbbrev(table.name)}
			</div>
			<Box style={{ flex: 1, minWidth: 0 }}>
				<Text size="xs" ff="monospace" truncate>
					{formatTableName(table.schema, table.name)}
				</Text>
				{table.rowEstimate != null && table.rowEstimate > 0 && (
					<Text size="xs" c="dimmed">
						~{table.rowEstimate.toLocaleString()}
					</Text>
				)}
			</Box>
		</UnstyledButton>
	);

	const renderCollapsedTable = (table: DbTableInfo) => (
		<Tooltip
			key={`${table.schema}.${table.name}`}
			label={formatTableName(table.schema, table.name)}
			position="right"
			openDelay={300}
		>
			<UnstyledButton
				className={classes.railItem}
				onClick={() => handleTableClick(table.schema, table.name)}
			>
				<div
					className={`${classes.railBadge} ${
						isSystemTable(table.name) ? classes.railBadgeSystem : classes.railBadgeFhir
					}`}
				>
					{getTableAbbrev(table.name)}
				</div>
			</UnstyledButton>
		</Tooltip>
	);

	if (expanded && selected) {
		return (
			<div className={`${classes.rail} ${classes.railExpanded}`}>
				<TableDetailView
					schema={selected.schema}
					table={selected.name}
					onBack={() => setSelected(null)}
				/>
			</div>
		);
	}

	return (
		<div className={`${classes.rail} ${expanded ? classes.railExpanded : ""}`}>
			{/* Header */}
			<div className={classes.railHeader}>
				{expanded ? (
					<Group justify="space-between" w="100%" px={4}>
						<Group gap={6}>
							<IconDatabase size={14} style={{ opacity: 0.5 }} />
							<Text size="xs" fw={600} c="dimmed">
								Tables
							</Text>
							{!isLoading && (
								<Text size="xs" c="dimmed">
									{tables.length}
								</Text>
							)}
						</Group>
						<Tooltip label="Collapse (Ctrl+B)">
							<ActionIcon variant="subtle" size="xs" onClick={onToggle}>
								<IconLayoutSidebarLeftCollapse size={14} />
							</ActionIcon>
						</Tooltip>
					</Group>
				) : (
					<Tooltip label="Expand schema (Ctrl+B)" position="right">
						<ActionIcon variant="subtle" size="xs" onClick={onToggle}>
							<IconLayoutSidebarLeftExpand size={14} />
						</ActionIcon>
					</Tooltip>
				)}
				</div>

			{/* Controls (expanded only) */}
			{expanded && (
				<Box className={classes.railControls}>
					<TextInput
						size="xs"
						placeholder="Search tables..."
						leftSection={<IconSearch size={13} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						key={searchFocusKey}
						autoFocus={!!searchFocusKey}
					/>
					<Group gap={6} wrap="nowrap">
						<SegmentedControl
							size="xs"
							fullWidth
							value={viewMode}
							onChange={(value) => setViewMode(value as RailViewMode)}
							data={[
								{ label: `All (${filtered.all.length})`, value: "all" },
								{ label: `FHIR (${filtered.fhir.length})`, value: "fhir" },
								{ label: `System (${filtered.system.length})`, value: "system" },
							]}
						/>
						<Select
							size="xs"
							w={102}
							value={sortMode}
							onChange={(value) => setSortMode((value as RailSortMode) ?? "rows")}
							data={[
								{ value: "rows", label: "By rows" },
								{ value: "name", label: "By name" },
							]}
							allowDeselect={false}
						/>
					</Group>
				</Box>
			)}

			{/* Content */}
			<ScrollArea className={classes.railContent}>
				{isLoading && (
					<Box ta="center" py="xl">
						<Loader size="sm" />
					</Box>
				)}

				{!isLoading && sortedActiveTables.length === 0 && (
					<Box className={classes.railEmpty}>
						<Text size="xs" c="dimmed">
							No tables found for current filters
						</Text>
					</Box>
				)}

				{!isLoading &&
					expanded &&
					groupedTables.map((group) => (
						<Box key={group.schema} className={classes.railSchemaGroup}>
							<Text size="xs" fw={600} c="dimmed" className={classes.railSchemaHeader}>
								{group.schema}
							</Text>
							{group.items.map(renderExpandedTable)}
						</Box>
					))}

				{!isLoading && !expanded && sortedActiveTables.map(renderCollapsedTable)}
			</ScrollArea>
		</div>
	);
}
