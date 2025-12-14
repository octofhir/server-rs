import { useState, useMemo } from "react";
import {
	Stack,
	Title,
	Text,
	Group,
	Button,
	Paper,
	Table,
	Alert,
	Code,
	ScrollArea,
	Collapse,
	UnstyledButton,
	Popover,
	Badge,
	Box,
} from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import { IconAlertCircle, IconInfoCircle, IconChevronDown, IconChevronRight, IconBraces } from "@tabler/icons-react";
import { SqlEditor } from "@/shared/monaco/SqlEditor";
import { useSqlMutation } from "@/shared/api/hooks";
import type { SqlValue, FhirOperationOutcome } from "@/shared/api/types";
import { ApiResponseError } from "@/shared/api/serverApi";

/** Component to display JSON values in a cell with popover for full view */
function JsonCell({ value }: { value: Record<string, unknown> }) {
	const [opened, { open, close }] = useDisclosure(false);
	const jsonString = JSON.stringify(value, null, 2);
	const preview = JSON.stringify(value);
	const isLarge = preview.length > 50;

	return (
		<Popover opened={opened} position="bottom-start" withArrow shadow="md" width={400}>
			<Popover.Target>
				<UnstyledButton
					onMouseEnter={open}
					onMouseLeave={close}
					style={{ display: "flex", alignItems: "center", gap: 4 }}
				>
					<IconBraces size={14} style={{ opacity: 0.5 }} />
					<Text size="sm" truncate style={{ maxWidth: 200, fontFamily: "var(--mantine-font-family-monospace)" }}>
						{isLarge ? `${preview.slice(0, 47)}...` : preview}
					</Text>
				</UnstyledButton>
			</Popover.Target>
			<Popover.Dropdown>
				<ScrollArea.Autosize mah={300}>
					<Code block style={{ fontSize: 12 }}>
						{jsonString}
					</Code>
				</ScrollArea.Autosize>
			</Popover.Dropdown>
		</Popover>
	);
}

/** Check if the result is from an EXPLAIN query */
function isExplainResult(columns: string[]): boolean {
	return columns.length === 1 && columns[0].toUpperCase() === "QUERY PLAN";
}

export function DbConsolePage() {
	const [query, setQuery] = useState("SELECT * FROM patient LIMIT 10;");
	const [showErrorDetails, setShowErrorDetails] = useState(false);
	const sqlMutation = useSqlMutation();

	const handleExecute = () => {
		sqlMutation.mutate({ query });
	};

	/** Check if this is an EXPLAIN result */
	const isExplain = useMemo(() => {
		if (!sqlMutation.data) return false;
		return isExplainResult(sqlMutation.data.columns);
	}, [sqlMutation.data]);

	/** Format EXPLAIN output as readable text */
	const explainText = useMemo(() => {
		if (!isExplain || !sqlMutation.data) return "";
		return sqlMutation.data.rows.map((row) => String(row[0])).join("\n");
	}, [isExplain, sqlMutation.data]);

	/** Render a cell value appropriately based on type */
	const renderCellValue = (value: SqlValue): React.ReactNode => {
		if (value === null) return <Text span c="dimmed" fs="italic">NULL</Text>;
		if (typeof value === "object" && value !== null) {
			return <JsonCell value={value as Record<string, unknown>} />;
		}
		if (typeof value === "boolean") {
			return <Badge size="xs" color={value ? "green" : "gray"}>{value.toString()}</Badge>;
		}
		return String(value);
	};

	// Extract error details
	const errorMessage = sqlMutation.error
		? sqlMutation.error instanceof ApiResponseError
			? sqlMutation.error.responseData?.resourceType === "OperationOutcome"
				? (sqlMutation.error.responseData as FhirOperationOutcome).issue?.[0]?.diagnostics ||
				  sqlMutation.error.message
				: sqlMutation.error.message
			: sqlMutation.error.message
		: null;

	const operationOutcome =
		sqlMutation.error instanceof ApiResponseError &&
		sqlMutation.error.responseData?.resourceType === "OperationOutcome"
			? (sqlMutation.error.responseData as FhirOperationOutcome)
			: null;

	return (
		<Stack gap="md" h="100%">
			<Group justify="space-between">
				<div>
					<Title order={2}>DB Console</Title>
					<Text c="dimmed" size="sm">
						Execute SQL queries against the database
					</Text>
				</div>
				<Button onClick={handleExecute} loading={sqlMutation.isPending}>
					Execute (Ctrl+Enter)
				</Button>
			</Group>

			<Paper withBorder p={0} style={{ flex: "0 0 300px", overflow: "hidden" }}>
				<Group px="sm" py="xs" bg="var(--mantine-color-gray-0)">
					<Text size="xs" fw={500} c="dimmed">
						SQL Editor
					</Text>
				</Group>
				<div style={{ height: "260px" }}>
					<SqlEditor
						value={query}
						onChange={setQuery}
						onExecute={handleExecute}
						enableLsp
					/>
				</div>
			</Paper>

			<Paper withBorder p="md" style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column" }}>
				<Group justify="space-between" mb="sm">
					<Text fw={500}>Results</Text>
					{sqlMutation.data && (
						<Text size="sm" c="dimmed">
							{sqlMutation.data.rowCount} rows in {sqlMutation.data.executionTimeMs}ms
						</Text>
					)}
				</Group>

				<div style={{ flex: 1, overflow: "auto" }}>
					{errorMessage && (
						<Stack gap="sm">
							<Alert icon={<IconAlertCircle size={16} />} color="red" title="Query Error">
								{errorMessage}
							</Alert>
							{operationOutcome && (
								<>
									<UnstyledButton
										onClick={() => setShowErrorDetails(!showErrorDetails)}
									>
										<Group gap="xs">
											{showErrorDetails ? (
												<IconChevronDown size={14} />
											) : (
												<IconChevronRight size={14} />
											)}
											<Text size="sm" c="dimmed">
												Show full OperationOutcome
											</Text>
										</Group>
									</UnstyledButton>
									<Collapse in={showErrorDetails}>
										<Code block>
											{JSON.stringify(operationOutcome, null, 2)}
										</Code>
									</Collapse>
								</>
							)}
						</Stack>
					)}

					{!sqlMutation.data && !sqlMutation.error && !sqlMutation.isPending && (
						<Text c="dimmed" ta="center" py="xl">
							Run a query to see results
						</Text>
					)}

					{sqlMutation.data?.rowCount === 0 && (
						<Alert icon={<IconInfoCircle size={16} />} color="blue">
							Query executed successfully. No rows returned.
						</Alert>
					)}

					{/* EXPLAIN output - render as formatted code block */}
					{sqlMutation.data && sqlMutation.data.rowCount > 0 && isExplain && (
						<Box>
							<Badge color="violet" size="sm" mb="sm">
								Query Plan
							</Badge>
							<ScrollArea>
								<Code block style={{ whiteSpace: "pre", fontFamily: "var(--mantine-font-family-monospace)", fontSize: 13 }}>
									{explainText}
								</Code>
							</ScrollArea>
						</Box>
					)}

					{/* Regular table results */}
					{sqlMutation.data && sqlMutation.data.rowCount > 0 && !isExplain && (
						<ScrollArea>
							<Table striped highlightOnHover withTableBorder>
								<Table.Thead>
									<Table.Tr>
										{sqlMutation.data.columns.map((col) => (
											<Table.Th key={col}>{col}</Table.Th>
										))}
									</Table.Tr>
								</Table.Thead>
								<Table.Tbody>
									{sqlMutation.data.rows.map((row, rowIdx) => (
										<Table.Tr key={rowIdx}>
											{row.map((cell, cellIdx) => (
												<Table.Td key={cellIdx}>
													{renderCellValue(cell)}
												</Table.Td>
											))}
										</Table.Tr>
									))}
								</Table.Tbody>
							</Table>
						</ScrollArea>
					)}
				</div>
			</Paper>
		</Stack>
	);
}
