import { Alert, Badge, DataPreview, Text } from "@octofhir/ui-kit";
import type { ReactNode } from "react";
import { CircleAlert as CircleExclamation, Info as CircleInfo } from "lucide-react";
import type { SqlResponse, SqlValue } from "@/shared/api/types";
import { JsonCellViewer } from "./JsonCellViewer";

interface ResultsTableProps {
	data: SqlResponse | undefined;
	error: Error | null;
	isPending: boolean;
}

function renderCellValue(value: SqlValue): ReactNode {
	if (value === null) {
		return (
			<Text as="span" c="dimmed" style={{ fontStyle: "italic" }}>
				NULL
			</Text>
		);
	}
	if (typeof value === "object" && value !== null) {
		return <JsonCellViewer value={value as Record<string, unknown>} />;
	}
	if (typeof value === "boolean") {
		return (
			<Badge size="xs" color={value ? "primary" : "deep"}>
				{value.toString()}
			</Badge>
		);
	}
	return String(value);
}

export function ResultsTable({ data, error, isPending }: ResultsTableProps) {
	if (isPending) {
		return (
			<Text c="dimmed" ta="center" py="xl" size="sm">
				Executing query...
			</Text>
		);
	}

	if (error) {
		return (
			<Alert icon={<CircleExclamation size={16} />} color="fire" title="Query Error">
				{error.message}
			</Alert>
		);
	}

	if (!data) {
		return (
			<Text c="dimmed" ta="center" py="xl" size="sm">
				Run a query to see results
			</Text>
		);
	}

	if (data.rowCount === 0) {
		return (
			<Alert icon={<CircleInfo size={16} />} color="primary">
				Query executed successfully. No rows returned.
			</Alert>
		);
	}

	return (
		<DataPreview
			columns={data.columns.map((column, columnIndex) => ({
				id: `${column}-${columnIndex}`,
				label: column,
				width: 160,
			}))}
			rows={data.rows.map((row) =>
				Object.fromEntries(
					data.columns.map((column, columnIndex) => [
						`${column}-${columnIndex}`,
						renderCellValue(row[columnIndex]),
					]),
				),
			)}
			getRowKey={(_row, rowIndex) => `${rowIndex}`}
			maxHeight={400}
		/>
	);
}
