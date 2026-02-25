import { useEffect, useMemo, useState } from "react";
import {
	ActionIcon,
	Box,
	Group,
	Loader,
	ScrollArea,
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

function isSystemTable(name: string): boolean {
	return name.startsWith("_");
}

function getTableAbbrev(name: string): string {
	const clean = name.startsWith("_") ? name.slice(1) : name;
	return clean.slice(0, 2);
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
		if (!search.trim()) return { fhir: fhirTables, system: systemTables };
		const q = search.toLowerCase();
		return {
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
	}, [fhirTables, systemTables, search]);

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

			{/* Search (expanded only) */}
			{expanded && (
				<Box px={8} py={4} style={{ flexShrink: 0 }}>
					<TextInput
						size="xs"
						placeholder="Search tables..."
						leftSection={<IconSearch size={13} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						key={searchFocusKey}
						autoFocus={!!searchFocusKey}
					/>
				</Box>
			)}

			{/* Content */}
			<ScrollArea className={classes.railContent}>
				{isLoading && (
					<Box ta="center" py="xl">
						<Loader size="sm" />
					</Box>
				)}

				{/* FHIR resource tables */}
				{filtered.fhir.map((t) =>
					expanded ? (
						<UnstyledButton
							key={`${t.schema}.${t.name}`}
							className={`${classes.railItem} ${classes.railItemExpanded}`}
							onClick={() => handleTableClick(t.schema, t.name)}
							onDoubleClick={() => handleInsertSelect(t.schema, t.name)}
						>
							<div
								className={`${classes.railBadge} ${classes.railBadgeFhir}`}
							>
								{getTableAbbrev(t.name)}
							</div>
							<Box style={{ flex: 1, minWidth: 0 }}>
								<Text size="xs" ff="monospace" truncate>
									{t.schema !== "public" ? `${t.schema}.` : ""}
									{t.name}
								</Text>
								{t.rowEstimate != null && t.rowEstimate > 0 && (
									<Text size="xs" c="dimmed">
										~{t.rowEstimate.toLocaleString()}
									</Text>
								)}
							</Box>
						</UnstyledButton>
					) : (
						<Tooltip
							key={`${t.schema}.${t.name}`}
							label={t.name}
							position="right"
							openDelay={300}
						>
							<UnstyledButton
								className={classes.railItem}
								onClick={() => handleTableClick(t.schema, t.name)}
							>
								<div
									className={`${classes.railBadge} ${classes.railBadgeFhir}`}
								>
									{getTableAbbrev(t.name)}
								</div>
							</UnstyledButton>
						</Tooltip>
					),
				)}

				{/* Separator */}
				{filtered.system.length > 0 && filtered.fhir.length > 0 && (
					<div className={classes.railSeparator} />
				)}

				{/* System tables */}
				{filtered.system.map((t) =>
					expanded ? (
						<UnstyledButton
							key={`${t.schema}.${t.name}`}
							className={`${classes.railItem} ${classes.railItemExpanded}`}
							onClick={() => handleTableClick(t.schema, t.name)}
							onDoubleClick={() => handleInsertSelect(t.schema, t.name)}
						>
							<div
								className={`${classes.railBadge} ${classes.railBadgeSystem}`}
							>
								{getTableAbbrev(t.name)}
							</div>
							<Box style={{ flex: 1, minWidth: 0 }}>
								<Text size="xs" ff="monospace" truncate>
									{t.name}
								</Text>
								{t.rowEstimate != null && t.rowEstimate > 0 && (
									<Text size="xs" c="dimmed">
										~{t.rowEstimate.toLocaleString()}
									</Text>
								)}
							</Box>
						</UnstyledButton>
					) : (
						<Tooltip
							key={`${t.schema}.${t.name}`}
							label={t.name}
							position="right"
							openDelay={300}
						>
							<UnstyledButton
								className={classes.railItem}
								onClick={() => handleTableClick(t.schema, t.name)}
							>
								<div
									className={`${classes.railBadge} ${classes.railBadgeSystem}`}
								>
									{getTableAbbrev(t.name)}
								</div>
							</UnstyledButton>
						</Tooltip>
					),
				)}
			</ScrollArea>
		</div>
	);
}
