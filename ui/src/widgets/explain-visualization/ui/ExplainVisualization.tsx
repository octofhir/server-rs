import { useState, useMemo } from "react";
import {
	Box,
	Text,
	Group,
	Badge,
	Stack,
	UnstyledButton,
	Code,
	useMantineTheme,
	useMantineColorScheme,
} from "@mantine/core";
import {
	IconChevronRight,
	IconChevronDown,
	IconSearch,
	IconTable,
	IconFilter,
	IconArrowsSort,
	IconCircleDot,
} from "@tabler/icons-react";

interface ExplainNode {
	operation: string;
	details: string;
	additionalDetails: string[]; // Multi-line details like Filter, Index Cond, etc.
	cost?: { startup: number; total: number };
	rows?: number;
	width?: number;
	actualTime?: { startup: number; total: number };
	actualRows?: number;
	loops?: number;
	children: ExplainNode[];
	indent: number;
	rawLine: string;
}

interface ExplainMetadata {
	planningTime?: number;
	executionTime?: number;
	totalRuntime?: number;
	triggers?: string[];
}

interface ExplainVisualizationProps {
	/** Raw EXPLAIN output text */
	explainText: string;
}

/**
 * Parse PostgreSQL EXPLAIN output into a tree structure
 */
function parseExplainText(text: string): { nodes: ExplainNode[]; metadata: ExplainMetadata } {
	const lines = text.split("\n").filter((line) => line.trim());
	const nodes: ExplainNode[] = [];
	const stack: { node: ExplainNode; indent: number }[] = [];
	const metadata: ExplainMetadata = {};

	for (const line of lines) {
		// Check for metadata lines (Planning Time, Execution Time, etc.)
		const planningTimeMatch = line.match(/Planning Time:\s+([\d.]+)\s+ms/);
		if (planningTimeMatch) {
			metadata.planningTime = Number.parseFloat(planningTimeMatch[1]);
			continue;
		}

		const executionTimeMatch = line.match(/Execution Time:\s+([\d.]+)\s+ms/);
		if (executionTimeMatch) {
			metadata.executionTime = Number.parseFloat(executionTimeMatch[1]);
			continue;
		}

		const totalRuntimeMatch = line.match(/Total runtime:\s+([\d.]+)\s+ms/);
		if (totalRuntimeMatch) {
			metadata.totalRuntime = Number.parseFloat(totalRuntimeMatch[1]);
			continue;
		}

		// Check for trigger information
		if (line.includes("Trigger")) {
			if (!metadata.triggers) metadata.triggers = [];
			metadata.triggers.push(line.trim());
			continue;
		}
		// Calculate indentation level
		const match = line.match(/^(\s*)(->)?\s*(.+)$/);
		if (!match) continue;

		const indent = match[1].length + (match[2] ? 3 : 0);
		const content = match[3];

		// Extract operation type and details
		const operationMatch = content.match(/^([^(]+)\s*(\(.*\))?(.*)$/);
		if (!operationMatch) continue;

		const operation = operationMatch[1].trim();
		const paramsText = operationMatch[2] || "";
		const details = operationMatch[3] || "";

		// Extract cost, rows, width
		const costMatch = paramsText.match(
			/cost=([\d.]+)\.\.([\d.]+)\s+rows=(\d+)\s+width=(\d+)/,
		);
		const actualTimeMatch = content.match(
			/actual time=([\d.]+)\.\.([\d.]+)\s+rows=(\d+)\s+loops=(\d+)/,
		);
		// Fallback without loops
		const actualTimeNoLoopsMatch = !actualTimeMatch ? content.match(
			/actual time=([\d.]+)\.\.([\d.]+)\s+rows=(\d+)/,
		) : null;

		const timeMatch = actualTimeMatch || actualTimeNoLoopsMatch;

		// Check if this is an additional detail line (Filter, Index Cond, etc.)
		// These don't have cost info and are indented under a parent node
		const isDetailLine = !costMatch && (
			operation.startsWith("Filter:") ||
			operation.startsWith("Index Cond:") ||
			operation.startsWith("Recheck Cond:") ||
			operation.startsWith("Output:") ||
			operation.startsWith("Buffers:") ||
			operation.startsWith("Sort Key:") ||
			operation.startsWith("Sort Method:") ||
			operation.startsWith("Join Filter:") ||
			operation.startsWith("Hash Cond:") ||
			operation.startsWith("Merge Cond:")
		);

		if (isDetailLine && stack.length > 0) {
			// Attach to the most recent node as additional detail
			const parentNode = stack[stack.length - 1].node;
			parentNode.additionalDetails.push(content.trim());
			continue;
		}

		const node: ExplainNode = {
			operation,
			details: details.trim(),
			additionalDetails: [],
			cost: costMatch
				? {
						startup: Number.parseFloat(costMatch[1]),
						total: Number.parseFloat(costMatch[2]),
					}
				: undefined,
			rows: costMatch ? Number.parseInt(costMatch[3], 10) : undefined,
			width: costMatch ? Number.parseInt(costMatch[4], 10) : undefined,
			actualTime: timeMatch
				? {
						startup: Number.parseFloat(timeMatch[1]),
						total: Number.parseFloat(timeMatch[2]),
					}
				: undefined,
			actualRows: timeMatch ? Number.parseInt(timeMatch[3], 10) : undefined,
			loops: actualTimeMatch ? Number.parseInt(actualTimeMatch[4], 10) : undefined,
			children: [],
			indent,
			rawLine: line,
		};

		// Build tree structure based on indentation
		while (stack.length > 0 && stack[stack.length - 1].indent >= indent) {
			stack.pop();
		}

		if (stack.length === 0) {
			nodes.push(node);
		} else {
			stack[stack.length - 1].node.children.push(node);
		}

		stack.push({ node, indent });
	}

	return { nodes, metadata };
}

/**
 * Get color for operation type
 */
function getOperationColor(operation: string): string {
	const op = operation.toLowerCase();

	// Expensive operations (red)
	if (op.includes("seq scan") || op.includes("sequential scan")) return "red";

	// Index operations (green)
	if (op.includes("index") && op.includes("scan")) return "green";

	// Join operations (blue)
	if (op.includes("join") || op.includes("nested loop")) return "blue";

	// Sort operations (yellow)
	if (op.includes("sort") || op.includes("order")) return "yellow";

	// Aggregate operations (violet)
	if (op.includes("aggregate") || op.includes("group")) return "violet";

	// Filter/condition operations (cyan)
	if (op.includes("filter") || op.includes("cond")) return "cyan";

	// Default (gray)
	return "gray";
}

/**
 * Get icon for operation type
 */
function getOperationIcon(operation: string) {
	const op = operation.toLowerCase();
	const size = 14;

	if (op.includes("scan")) return <IconSearch size={size} />;
	if (op.includes("join")) return <IconTable size={size} />;
	if (op.includes("filter")) return <IconFilter size={size} />;
	if (op.includes("sort")) return <IconArrowsSort size={size} />;

	return <IconCircleDot size={size} />;
}

/**
 * Individual tree node component
 */
function ExplainTreeNode({ node }: { node: ExplainNode }) {
	const theme = useMantineTheme();
	const { colorScheme } = useMantineColorScheme();
	const [expanded, setExpanded] = useState(true);

	const hasChildren = node.children.length > 0;
	const color = getOperationColor(node.operation);

	return (
		<Box>
			<UnstyledButton
				onClick={() => hasChildren && setExpanded(!expanded)}
				style={{
					width: "100%",
					padding: "6px 8px",
					borderRadius: theme.radius.sm,
					cursor: hasChildren ? "pointer" : "default",
				}}
				styles={{
					root: {
						"&:hover": hasChildren
							? {
									backgroundColor:
										colorScheme === "dark"
											? theme.colors.dark[6]
											: theme.colors.gray[1],
								}
							: {},
					},
				}}
			>
				<Group gap="xs" wrap="nowrap">
					{/* Expand/collapse icon */}
					<Box style={{ width: 16, height: 16, flexShrink: 0 }}>
						{hasChildren &&
							(expanded ? (
								<IconChevronDown size={16} />
							) : (
								<IconChevronRight size={16} />
							))}
					</Box>

					{/* Operation icon */}
					<Box style={{ flexShrink: 0 }}>{getOperationIcon(node.operation)}</Box>

					{/* Operation name */}
					<Badge size="sm" color={color} variant="light">
						{node.operation}
					</Badge>

					{/* Cost and rows */}
					{node.cost && (
						<Group gap={4}>
							<Text size="xs" c="dimmed">
								cost={node.cost.startup.toFixed(2)}..{node.cost.total.toFixed(2)}
							</Text>
							<Text size="xs" c="dimmed">
								rows={node.rows}
							</Text>
						</Group>
					)}

					{/* Actual time (from EXPLAIN ANALYZE) */}
					{node.actualTime && (
						<Group gap={4}>
							<Text size="xs" c="teal">
								actual={node.actualTime.startup.toFixed(2)}..
								{node.actualTime.total.toFixed(2)}
							</Text>
							<Text size="xs" c="teal">
								rows={node.actualRows}
							</Text>
							{node.loops && node.loops > 1 && (
								<Text size="xs" c="teal">
									loops={node.loops}
								</Text>
							)}
						</Group>
					)}
				</Group>

				{/* Details - Show full text without truncation */}
				{node.details && (
					<Box mt={4} ml={40}>
						<Text size="xs" c="dimmed" style={{ wordBreak: "break-word", whiteSpace: "pre-wrap" }}>
							{node.details}
						</Text>
					</Box>
				)}

				{/* Additional details - Filter, Index Cond, etc. */}
				{node.additionalDetails.length > 0 && (
					<Box mt={4} ml={40}>
						<Stack gap={2}>
							{node.additionalDetails.map((detail, idx) => (
								<Text
									key={`${detail.substring(0, 50)}-${idx}`}
									size="xs"
									c="dimmed"
									style={{ wordBreak: "break-word", whiteSpace: "pre-wrap" }}
								>
									{detail}
								</Text>
							))}
						</Stack>
					</Box>
				)}
			</UnstyledButton>

			{/* Children */}
			{hasChildren && expanded && (
				<Box pl={24} mt={4}>
					<Stack gap={2}>
						{node.children.map((child, index) => (
							<ExplainTreeNode
								key={`${child.operation}-${index}-${child.rawLine}`}
								node={child}
							/>
						))}
					</Stack>
				</Box>
			)}
		</Box>
	);
}

/**
 * EXPLAIN query plan visualization component
 * Renders PostgreSQL EXPLAIN output as an interactive tree
 */
export function ExplainVisualization({
	explainText,
}: ExplainVisualizationProps) {
	const theme = useMantineTheme();
	const { colorScheme } = useMantineColorScheme();

	const { nodes, metadata } = useMemo(() => parseExplainText(explainText), [explainText]);

	if (nodes.length === 0) {
		return (
			<Box
				p="md"
				style={{
					backgroundColor:
						colorScheme === "dark"
							? theme.colors.dark[6]
							: theme.colors.gray[0],
					borderRadius: theme.radius.md,
				}}
			>
				<Text c="dimmed" size="sm" ta="center">
					Unable to parse EXPLAIN output
				</Text>
				<Code block mt="md" style={{ fontSize: 12 }}>
					{explainText}
				</Code>
			</Box>
		);
	}

	return (
		<Box>
			<Group gap="xs" mb="md">
				<Badge color="violet" size="sm">
					Query Plan
				</Badge>
				<Text size="xs" c="dimmed">
					{nodes.length} operation{nodes.length !== 1 ? "s" : ""}
				</Text>
			</Group>
			<Box
				p="sm"
				style={{
					backgroundColor:
						colorScheme === "dark"
							? theme.colors.dark[6]
							: theme.colors.gray[0],
					borderRadius: theme.radius.md,
				}}
			>
				<Stack gap={2}>
					{nodes.map((node, index) => (
						<ExplainTreeNode
							key={`${node.operation}-${index}-${node.rawLine}`}
							node={node}
						/>
					))}
				</Stack>
			</Box>

			{/* Display metadata (Planning Time, Execution Time, etc.) */}
			{(metadata.planningTime || metadata.executionTime || metadata.totalRuntime || metadata.triggers) && (
				<Box mt="md" p="md" style={{
					backgroundColor: colorScheme === "dark" ? theme.colors.dark[6] : theme.colors.gray[0],
					borderRadius: theme.radius.md,
				}}>
					<Stack gap="sm">
						{metadata.planningTime !== undefined && (
							<Group gap="xs">
								<Text size="sm" fw={600} c="dimmed">Planning Time:</Text>
								<Text size="lg" fw={700} c={colorScheme === "dark" ? "blue.4" : "blue.7"}>
									{metadata.planningTime.toFixed(3)} ms
								</Text>
							</Group>
						)}
						{metadata.executionTime !== undefined && (
							<Group gap="xs">
								<Text size="sm" fw={600} c="dimmed">Execution Time:</Text>
								<Text size="lg" fw={700} c={colorScheme === "dark" ? "teal.4" : "teal.7"}>
									{metadata.executionTime.toFixed(3)} ms
								</Text>
							</Group>
						)}
						{metadata.totalRuntime !== undefined && (
							<Group gap="xs">
								<Text size="sm" fw={600} c="dimmed">Total Runtime:</Text>
								<Text size="md" fw={600} c={colorScheme === "dark" ? "violet.4" : "violet.7"}>
									{metadata.totalRuntime.toFixed(3)} ms
								</Text>
							</Group>
						)}
						{metadata.triggers && metadata.triggers.length > 0 && (
							<Box>
								<Text size="xs" fw={500} c="dimmed" mb={4}>Triggers:</Text>
								{metadata.triggers.map((trigger) => (
									<Text key={trigger} size="xs" c="dimmed" ml="md">{trigger}</Text>
								))}
							</Box>
						)}
					</Stack>
				</Box>
			)}
		</Box>
	);
}
