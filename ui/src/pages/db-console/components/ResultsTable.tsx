import { Table, Text, Badge, ScrollArea, Alert } from "@/shared/ui";
import { IconAlertCircle, IconInfoCircle } from "@tabler/icons-react";
import type { SqlResponse, SqlValue } from "@/shared/api/types";
import { JsonCellViewer } from "./JsonCellViewer";

interface ResultsTableProps {
	data: SqlResponse | undefined;
	error: Error | null;
	isPending: boolean;
}

function renderCellValue(value: SqlValue): React.ReactNode {
	if (value === null) {
		return (
			<Text span c="dimmed" fs="italic">
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
			<Alert icon={<IconAlertCircle size={16} />} color="fire" title="Query Error">
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
			<Alert icon={<IconInfoCircle size={16} />} color="primary">
				Query executed successfully. No rows returned.
			</Alert>
		);
	}

	return (
		<ScrollArea>
			<Table striped highlightOnHover>
				<Table.Thead>
					<Table.Tr>
						{data.columns.map((col) => (
							<Table.Th key={col}>{col}</Table.Th>
						))}
					</Table.Tr>
				</Table.Thead>
				<Table.Tbody>
					{data.rows.map((row, rowIdx) => (
						<Table.Tr key={rowIdx}>
							{row.map((cell, cellIdx) => (
								<Table.Td key={cellIdx}>{renderCellValue(cell)}</Table.Td>
							))}
						</Table.Tr>
					))}
				</Table.Tbody>
			</Table>
		</ScrollArea>
	);
}
