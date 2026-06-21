import { Badge, Code, Collapse, Text } from "@octofhir/ui-kit";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";
import type { FhirPathResult } from "../types";
import classes from "../FhirPathConsolePage.module.css";

interface Props {
	result: FhirPathResult;
}

export function ResultItem({ result }: Props) {
	const [expanded, setExpanded] = useState(false);
	const isComplex = typeof result.value === "object" && result.value !== null;

	const typeColor = getTypeColor(result.datatype);

	return (
		<div className={classes.resultItem}>
			<div className={classes.resultItemContent}>
				<div className={classes.resultHeader}>
					<Text size="xs" c="dimmed">
						[{result.index}]
					</Text>
					<Badge size="sm" color={typeColor}>
						{result.datatype}
					</Badge>

					{isComplex ? (
						<button
							type="button"
							onClick={() => setExpanded(!expanded)}
							className={classes.resultToggle}
							aria-expanded={expanded}
							aria-label={`${expanded ? "Collapse" : "Expand"} ${result.datatype} object`}
						>
							<span aria-hidden="true">
								{expanded ? (
									<ChevronDown size={14} />
								) : (
									<ChevronRight size={14} />
								)}
							</span>
							<Text size="sm" c="dimmed">
								{result.datatype} object
							</Text>
						</button>
					) : (
						<Code className={classes.resultCode}>
							{formatPrimitiveValue(result.value)}
						</Code>
					)}
				</div>

				{isComplex && (
					<Collapse in={expanded}>
						<Code block className={classes.resultJson}>
							{JSON.stringify(result.value, null, 2)}
						</Code>
					</Collapse>
				)}
			</div>
		</div>
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
