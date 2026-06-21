import { useCallback, useMemo, useState } from "react";
import {
	Text,
	Badge,
	ActionIcon,
	Tooltip,
	Collapse,
	UnstyledButton,
} from "@octofhir/ui-kit";
import { ChevronDown, ChevronRight, Copy, ArrowDownToLine, Play, LayoutDashboard as ChartTreemap, X as Xmark } from "lucide-react";
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
					className={[
						classes.entryQueryCode,
						isLongQuery && !queryExpanded ? classes.queryClamp3 : undefined,
					].filter(Boolean).join(" ")}
					onClick={() => onReplayQuery(entry.query)}
				>
					{entry.query}
				</UnstyledButton>
			</Tooltip>

			{/* Long query expand toggle */}
			{isLongQuery && (
				<div className={classes.entryBlock}>
					<UnstyledButton onClick={() => setQueryExpanded((v) => !v)}>
						<Text size="xs" c="dimmed">
							{queryExpanded ? "Collapse" : "Show full query..."}
						</Text>
					</UnstyledButton>
				</div>
			)}

			{/* Meta + actions row */}
			<div className={classes.entryMeta}>
				<div className={classes.entryMetaContent}>
					{entry.status === "pending" && (
						<Badge size="xs" variant="light" color="warm">
							running...
						</Badge>
					)}
					{entry.executionTimeMs != null && (
						<Text size="xs" c="dimmed" className={classes.nowrap}>
							{entry.executionTimeMs}ms
						</Text>
					)}
					{entry.result && (
						<Text size="xs" c="dimmed" className={classes.nowrap}>
							{entry.result.rowCount} rows
						</Text>
					)}
					<Text size="xs" c="dimmed" ff="monospace" className={classes.nowrap}>
						{formatTime(entry.timestamp)}
					</Text>
				</div>

				{/* Actions */}
				{entry.status !== "pending" && (
					<div className={classes.entryActions}>
						<Tooltip label="Copy query">
							<ActionIcon variant="subtle" size="xs" onClick={handleCopy}>
								<Copy size={12} />
							</ActionIcon>
						</Tooltip>
						<Tooltip label="Re-run">
							<ActionIcon
								variant="subtle"
								size="xs"
								onClick={() => onReplayQuery(entry.query)}
							>
								<Play size={12} />
							</ActionIcon>
						</Tooltip>
						{hasResults && (
							<Tooltip label="Export CSV">
								<ActionIcon variant="subtle" size="xs" onClick={handleExport}>
									<ArrowDownToLine size={12} />
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
									<ChartTreemap size={12} />
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
									<ChevronDown size={13} />
								) : (
									<ChevronRight size={13} />
								)}
							</ActionIcon>
						)}
						<Tooltip label="Remove">
							<ActionIcon
								variant="subtle"
								size="xs"
								onClick={() => onRemoveEntry(entry.id)}
							>
								<Xmark size={12} />
							</ActionIcon>
						</Tooltip>
					</div>
				)}
			</div>

			{/* Error */}
			{entry.status === "error" && entry.error && (
				<div className={classes.entryBlockTight}>
					<Text
						size="xs"
						ff="monospace"
						c="var(--octo-accent-fire)"
						className={classes.preWrap}
					>
						{entry.error}
					</Text>
				</div>
			)}

			{/* Results */}
			{entry.status === "success" && entry.result && (
				<Collapse in={shouldShow}>
					<div className={classes.entryBlockTight}>
						<div className={classes.entryResultPane}>
							<ResultsTable
								data={entry.result}
								error={null}
								isPending={false}
							/>
						</div>
					</div>
				</Collapse>
			)}

			{/* Pending state */}
			{entry.status === "pending" && (
				<div className={classes.entryBlockTight}>
					<Text size="xs" c="dimmed">
						Executing...
					</Text>
				</div>
			)}

			{/* Explain */}
			{hasExplain && showExplain && (
				<div className={classes.entryBlockTight}>
					<div className={classes.entryResultPane}>
						<ExplainPane
							data={entry.explainData}
							error={null}
							isPending={false}
						/>
					</div>
				</div>
			)}
		</div>
	);
}
