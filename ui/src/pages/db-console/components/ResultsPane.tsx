import { useState, useCallback } from "react";
import { Tabs, Group, Text, Badge, ActionIcon, Tooltip, Box } from "@/shared/ui";
import { IconDownload } from "@tabler/icons-react";
import type { SqlResponse, SqlValue } from "@/shared/api/types";
import { ResultsTable } from "./ResultsTable";
import { ExplainPane } from "./ExplainPane";

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
		<Box style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
			<Tabs
				value={activeTab}
				onChange={(v) => setActiveTab(v ?? "results")}
				variant="outline"
				style={{ display: "flex", flexDirection: "column", height: "100%" }}
			>
				<Group justify="space-between" px="sm" style={{ flexShrink: 0, borderBottom: "1px solid var(--octo-border-subtle)" }}>
					<Tabs.List style={{ border: "none" }}>
						<Tabs.Tab value="results">
							<Group gap={6}>
								<Text size="xs">Results</Text>
								{sqlData && (
									<Badge size="xs" variant="light">
										{sqlData.rowCount} rows
									</Badge>
								)}
							</Group>
						</Tabs.Tab>
						{hasExplain && (
							<Tabs.Tab value="explain">
								<Group gap={6}>
									<Text size="xs">Explain</Text>
									{explainPending && (
										<Badge size="xs" variant="light" color="warm">
											...
										</Badge>
									)}
								</Group>
							</Tabs.Tab>
						)}
					</Tabs.List>

					<Group gap={4}>
						{sqlData && (
							<Text size="xs" c="dimmed">
								{sqlData.executionTimeMs}ms
							</Text>
						)}
						{sqlData && sqlData.rowCount > 0 && (
							<Tooltip label="Export to CSV">
								<ActionIcon variant="subtle" size="xs" onClick={handleExport}>
									<IconDownload size={14} />
								</ActionIcon>
							</Tooltip>
						)}
					</Group>
				</Group>

				<Box style={{ flex: 1, overflow: "auto", padding: "var(--mantine-spacing-sm)" }}>
					<Tabs.Panel value="results" style={{ height: "100%" }}>
						<ResultsTable data={sqlData} error={sqlError} isPending={sqlPending} />
					</Tabs.Panel>
					{hasExplain && (
						<Tabs.Panel value="explain" style={{ height: "100%" }}>
							<ExplainPane data={explainData} error={explainError} isPending={explainPending} />
						</Tabs.Panel>
					)}
				</Box>
			</Tabs>
		</Box>
	);
}
