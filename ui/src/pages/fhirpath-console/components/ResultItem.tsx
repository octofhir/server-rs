import { Badge, Code, Collapse, Group, Paper, Stack, Text } from "@mantine/core";
import { IconChevronDown, IconChevronRight } from "@tabler/icons-react";
import { useState } from "react";
import type { FhirPathResult } from "../types";

interface Props {
	result: FhirPathResult;
}

export function ResultItem({ result }: Props) {
	const [expanded, setExpanded] = useState(false);
	const isComplex = typeof result.value === "object" && result.value !== null;

	const typeColor = getTypeColor(result.datatype);

	return (
		<Paper withBorder p="sm" bg="var(--app-surface-2)">
			<Stack gap="xs">
				<Group gap="xs" align="center">
					<Text size="xs" c="dimmed">
						[{result.index}]
					</Text>
					<Badge size="sm" color={typeColor}>
						{result.datatype}
					</Badge>

					{isComplex ? (
						<Group
							gap="xs"
							onClick={() => setExpanded(!expanded)}
							style={{ cursor: "pointer", flex: 1 }}
						>
							{expanded ? (
								<IconChevronDown size={14} />
							) : (
								<IconChevronRight size={14} />
							)}
							<Text size="sm" c="dimmed">
								{result.datatype} object
							</Text>
						</Group>
					) : (
						<Code style={{ flex: 1 }}>
							{formatPrimitiveValue(result.value)}
						</Code>
					)}
				</Group>

				{isComplex && (
					<Collapse in={expanded}>
						<Code block style={{ maxHeight: 400, overflow: "auto" }}>
							{JSON.stringify(result.value, null, 2)}
						</Code>
					</Collapse>
				)}
			</Stack>
		</Paper>
	);
}

function getTypeColor(datatype: string): string {
	if (
		datatype === "string" ||
		datatype === "code" ||
		datatype === "id" ||
		datatype === "uri" ||
		datatype === "url"
	)
		return "green";
	if (datatype === "integer" || datatype === "decimal") return "blue";
	if (datatype === "boolean") return "orange";
	if (
		datatype === "date" ||
		datatype === "dateTime" ||
		datatype === "time"
	)
		return "cyan";
	return "violet"; // Complex types
}

function formatPrimitiveValue(value: unknown): string {
	if (typeof value === "string") return `"${value}"`;
	if (typeof value === "boolean") return value ? "true" : "false";
	if (typeof value === "number") return value.toString();
	return String(value);
}
