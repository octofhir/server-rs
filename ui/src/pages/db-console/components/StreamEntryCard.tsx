import { useCallback, useMemo, useState } from "react";
import {
	Text,
	Group,
	Badge,
	ActionIcon,
	Tooltip,
	Box,
	Collapse,
	UnstyledButton,
} from "@/shared/ui";
import {
	IconChevronDown,
	IconChevronRight,
	IconCopy,
	IconDownload,
	IconPlayerPlay,
	IconChartTreemap,
	IconX,
} from "@tabler/icons-react";
import type { SqlResponse, SqlValue } from "@/shared/api/types";
import { ResultsTable } from "./ResultsTable";
import { ExplainPane } from "./ExplainPane";
import classes from "../DbConsolePage.module.css";

export interface StreamEntry {
	id: string;
	query: string;
	result?: SqlResponse;
	error?: string;
	explainData?: SqlResponse;
	executionTimeMs?: number;
	timestamp: Date;
	status: "success" | "error" | "pending";
	isExpanded: boolean;
	fromHistory?: boolean;
}

interface StreamEntryCardProps {
	entry: StreamEntry;
	onReplayQuery: (query: string) => void;
	onToggleExpand: (id: string) => void;
	onRemoveEntry: (id: string) => void;
}

function formatTime(date: Date): string {
	return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function quoteCSVField(value: string): string {
	if (value.includes(",") || value.includes('"') || value.includes("\n")) {
		return `"${value.replace(/"/g, '""')}"`;
	}
	return value;
}

function exportToCSV(columns: string[], rows: SqlValue[][]): void {
	const csvHeader = columns.map(quoteCSVField).join(",");
	const csvRows = rows.map((row) =>
		row
			.map((cell) => {
				if (cell === null) return "";
				if (typeof cell === "object") return JSON.stringify(cell);
				return quoteCSVField(String(cell));
			})
			.join(","),
	);

	const csv = [csvHeader, ...csvRows].join("\n");
	const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
	const link = document.createElement("a");
	const url = URL.createObjectURL(blob);
	link.setAttribute("href", url);
	link.setAttribute("download", `query-results-${Date.now()}.csv`);
	link.style.visibility = "hidden";
	document.body.appendChild(link);
	link.click();
	document.body.removeChild(link);
	URL.revokeObjectURL(url);
}

export function StreamEntryCard({
	entry,
	onReplayQuery,
	onToggleExpand,
	onRemoveEntry,
}: StreamEntryCardProps) {
	const [showExplain, setShowExplain] = useState(false);
	const [queryExpanded, setQueryExpanded] = useState(false);

	const isLongQuery = entry.query.split("\n").length > 3 || entry.query.length > 200;

	const statusClass =
		entry.status === "success"
			? classes.entryCardSuccess
			: entry.status === "error"
				? classes.entryCardError
				: classes.entryCardPending;

	const handleCopy = useCallback(() => {
		navigator.clipboard.writeText(entry.query);
	}, [entry.query]);

	const handleExport = useCallback(() => {
		if (entry.result && entry.result.rowCount > 0) {
			exportToCSV(entry.result.columns, entry.result.rows);
		}
	}, [entry.result]);

	const shouldShow = useMemo(() => {
		if (entry.status === "pending") return false;
		return entry.isExpanded;
	}, [entry.status, entry.isExpanded]);

	const hasResults =
		entry.result && entry.status === "success" && entry.result.rowCount > 0;
	const hasExplain = !!entry.explainData;

	return (
		<div className={`${classes.entryCard} ${statusClass}`}>
			{/* Query text */}
			<Tooltip label="Click to load into editor" openDelay={500}>
				<UnstyledButton
					className={classes.entryQueryCode}
					onClick={() => onReplayQuery(entry.query)}
					style={
						isLongQuery && !queryExpanded
							? {
									display: "-webkit-box",
									WebkitLineClamp: 3,
									WebkitBoxOrient: "vertical",
									overflow: "hidden",
								}
							: undefined
					}
				>
					{entry.query}
				</UnstyledButton>
			</Tooltip>

			{/* Long query expand toggle */}
			{isLongQuery && (
				<Box px={10}>
					<UnstyledButton onClick={() => setQueryExpanded((v) => !v)}>
						<Text size="xs" c="dimmed">
							{queryExpanded ? "Collapse" : "Show full query..."}
						</Text>
					</UnstyledButton>
				</Box>
			)}

			{/* Meta + actions row */}
			<div className={classes.entryMeta}>
				<Group gap={6} wrap="nowrap">
					{entry.status === "pending" && (
						<Badge size="xs" variant="light" color="warm">
							running...
						</Badge>
					)}
					{entry.executionTimeMs != null && (
						<Text size="xs" c="dimmed" style={{ whiteSpace: "nowrap" }}>
							{entry.executionTimeMs}ms
						</Text>
					)}
					{entry.result && (
						<Text size="xs" c="dimmed" style={{ whiteSpace: "nowrap" }}>
							{entry.result.rowCount} rows
						</Text>
					)}
					<Text size="xs" c="dimmed" ff="monospace" style={{ whiteSpace: "nowrap" }}>
						{formatTime(entry.timestamp)}
					</Text>
				</Group>

				{/* Actions */}
				{entry.status !== "pending" && (
					<Group gap={2} wrap="nowrap" className={classes.entryActions}>
						<Tooltip label="Copy query">
							<ActionIcon variant="subtle" size="xs" onClick={handleCopy}>
								<IconCopy size={12} />
							</ActionIcon>
						</Tooltip>
						<Tooltip label="Re-run">
							<ActionIcon
								variant="subtle"
								size="xs"
								onClick={() => onReplayQuery(entry.query)}
							>
								<IconPlayerPlay size={12} />
							</ActionIcon>
						</Tooltip>
						{hasResults && (
							<Tooltip label="Export CSV">
								<ActionIcon variant="subtle" size="xs" onClick={handleExport}>
									<IconDownload size={12} />
								</ActionIcon>
							</Tooltip>
						)}
						{hasExplain && (
							<Tooltip label={showExplain ? "Hide explain" : "Show explain"}>
								<ActionIcon
									variant="subtle"
									size="xs"
									onClick={() => setShowExplain((v) => !v)}
									color={showExplain ? "primary" : undefined}
								>
									<IconChartTreemap size={12} />
								</ActionIcon>
							</Tooltip>
						)}
						{hasResults && (
							<ActionIcon
								variant="subtle"
								size="xs"
								onClick={() => onToggleExpand(entry.id)}
							>
								{shouldShow ? (
									<IconChevronDown size={13} />
								) : (
									<IconChevronRight size={13} />
								)}
							</ActionIcon>
						)}
						<Tooltip label="Remove">
							<ActionIcon
								variant="subtle"
								size="xs"
								onClick={() => onRemoveEntry(entry.id)}
							>
								<IconX size={12} />
							</ActionIcon>
						</Tooltip>
					</Group>
				)}
			</div>

			{/* Error */}
			{entry.status === "error" && entry.error && (
				<Box px={10} pb={6}>
					<Text
						size="xs"
						ff="monospace"
						c="var(--octo-accent-fire)"
						style={{ whiteSpace: "pre-wrap" }}
					>
						{entry.error}
					</Text>
				</Box>
			)}

			{/* Results */}
			{entry.status === "success" && entry.result && (
				<Collapse in={shouldShow}>
					<Box px={10} pb={6}>
						<div style={{ maxHeight: 400, overflow: "auto" }}>
							<ResultsTable
								data={entry.result}
								error={null}
								isPending={false}
							/>
						</div>
					</Box>
				</Collapse>
			)}

			{/* Pending state */}
			{entry.status === "pending" && (
				<Box px={10} pb={6}>
					<Text size="xs" c="dimmed">
						Executing...
					</Text>
				</Box>
			)}

			{/* Explain */}
			{hasExplain && showExplain && (
				<Box px={10} pb={6}>
					<div style={{ maxHeight: 400, overflow: "auto" }}>
						<ExplainPane
							data={entry.explainData}
							error={null}
							isPending={false}
						/>
					</div>
				</Box>
			)}
		</div>
	);
}
