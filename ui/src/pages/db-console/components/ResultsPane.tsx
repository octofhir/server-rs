import { ActionIcon, Badge, Tabs, Text, Tooltip } from "@octofhir/ui-kit";
import { useState, useCallback } from "react";
import { ArrowDownToLine } from "lucide-react";
import type { SqlResponse, SqlValue } from "@/shared/api/types";
import { ResultsTable } from "./ResultsTable";
import { ExplainPane } from "./ExplainPane";
import classes from "../DbConsolePage.module.css";

interface ResultsPaneProps {
	sqlData: SqlResponse | undefined;
	sqlError: Error | null;
	sqlPending: boolean;
	explainData: SqlResponse | undefined;
	explainError: Error | null;
	explainPending: boolean;
}

function exportToCSV(columns: string[], rows: SqlValue[][]): void {
	const csvHeader = columns.join(",");
	const csvRows = rows.map((row) =>
		row
			.map((cell) => {
				if (cell === null) return "";
				if (typeof cell === "object") return JSON.stringify(cell);
				const str = String(cell);
				if (str.includes(",") || str.includes('"') || str.includes("\n")) {
					return `"${str.replace(/"/g, '""')}"`;
				}
				return str;
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
}

export function ResultsPane({
	sqlData,
	sqlError,
	sqlPending,
	explainData,
	explainError,
	explainPending,
}: ResultsPaneProps) {
	const [activeTab, setActiveTab] = useState<string>("results");

	const handleExport = useCallback(() => {
		if (sqlData && sqlData.rowCount > 0) {
			exportToCSV(sqlData.columns, sqlData.rows);
		}
	}, [sqlData]);

	const hasExplain = explainData || explainPending || explainError;

	return (
		<div className={classes.resultsPane}>
			<Tabs
				value={activeTab}
				onChange={(v) => setActiveTab(v ?? "results")}
				variant="outline"
				className={classes.resultsTabs}
			>
				<div className={classes.resultsHeader}>
					<Tabs.List className={classes.resultsTabsList}>
						<Tabs.Tab value="results">
							<div className={classes.tabLabel}>
								<Text size="xs">Results</Text>
								{sqlData && (
									<Badge size="xs" variant="light">
										{sqlData.rowCount} rows
									</Badge>
								)}
							</div>
						</Tabs.Tab>
						{hasExplain && (
							<Tabs.Tab value="explain">
								<div className={classes.tabLabel}>
									<Text size="xs">Explain</Text>
									{explainPending && (
										<Badge size="xs" variant="light" color="warm">
											...
										</Badge>
									)}
								</div>
							</Tabs.Tab>
						)}
					</Tabs.List>

					<div className={classes.resultsActions}>
						{sqlData && (
							<Text size="xs" c="dimmed">
								{sqlData.executionTimeMs}ms
							</Text>
						)}
						{sqlData && sqlData.rowCount > 0 && (
							<Tooltip label="Export to CSV">
								<ActionIcon variant="subtle" size="xs" onClick={handleExport}>
									<ArrowDownToLine size={14} />
								</ActionIcon>
							</Tooltip>
						)}
					</div>
				</div>

				<div className={classes.resultsBody}>
					<Tabs.Panel value="results" className={classes.resultsPanel}>
						<ResultsTable data={sqlData} error={sqlError} isPending={sqlPending} />
					</Tabs.Panel>
					{hasExplain && (
						<Tabs.Panel value="explain" className={classes.resultsPanel}>
							<ExplainPane data={explainData} error={explainError} isPending={explainPending} />
						</Tabs.Panel>
					)}
				</div>
			</Tabs>
		</div>
	);
}
