import { ActionIcon, Badge, Collapse, CopyButton, Text, Tooltip } from "@/shared/ui";
import { useState, memo } from "react";

import {
	ChevronRight,
	ChevronDown,
	Copy,
	Check,
} from "@gravity-ui/icons";
import type { LogEntry as LogEntryType, LogLevel } from "@/shared/api/types";
import classes from "./LogEntry.module.css";

interface LogEntryProps {
	entry: LogEntryType;
}

const LEVEL_COLORS: Record<LogLevel, string> = {
	trace: "gray",
	debug: "primary",
	info: "primary",
	warn: "warm",
	error: "fire",
};

const LEVEL_VARIANT: Record<LogLevel, "light" | "filled"> = {
	trace: "light",
	debug: "light",
	info: "light",
	warn: "light",
	error: "filled",
};

function formatTimestamp(timestamp: string): string {
	const date = new Date(timestamp);
	return date.toLocaleTimeString("en-US", {
		hour12: false,
		hour: "2-digit",
		minute: "2-digit",
		second: "2-digit",
		fractionalSecondDigits: 3,
	});
}

function formatFullTimestamp(timestamp: string): string {
	const date = new Date(timestamp);
	return date.toLocaleString("en-US", {
		year: "numeric",
		month: "short",
		day: "2-digit",
		hour12: false,
		hour: "2-digit",
		minute: "2-digit",
		second: "2-digit",
		fractionalSecondDigits: 3,
	});
}

function formatFieldValue(value: unknown): string {
	if (typeof value === "object" && value !== null) {
		return JSON.stringify(value);
	}
	return String(value);
}

function LogEntryComponent({ entry }: LogEntryProps) {
	const [expanded, setExpanded] = useState(false);
	const hasDetails = entry.fields || entry.span;

	const copyContent = JSON.stringify(entry, null, 2);

	return (
		<div className={classes.entry} data-level={entry.level}>
			<div
				className={`${classes.mainRow} ${hasDetails ? classes.clickableRow : ""}`}
				onClick={() => hasDetails && setExpanded(!expanded)}
			>
				{hasDetails ? (
					<ActionIcon
						variant="subtle"
						size="xs"
						color="gray"
						className={classes.expandIcon}
					>
						{expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
					</ActionIcon>
				) : (
					<span className={classes.expandSpacer} />
				)}

				<Tooltip label={formatFullTimestamp(entry.timestamp)} position="top" withArrow>
					<Text size="xs" c="dimmed" className={classes.timestamp} ff="monospace">
						{formatTimestamp(entry.timestamp)}
					</Text>
				</Tooltip>

				<Badge
					size="xs"
					color={LEVEL_COLORS[entry.level]}
					variant={LEVEL_VARIANT[entry.level]}
					className={classes.levelBadge}
				>
					{entry.level.toUpperCase()}
				</Badge>

				<Text size="xs" c="dimmed" className={classes.target} ff="monospace" truncate>
					{entry.target}
				</Text>

				<Text size="sm" className={classes.message} ff="monospace" truncate>
					{entry.message}
				</Text>

				<CopyButton value={copyContent} timeout={2000}>
					{({ copied, copy }) => (
						<Tooltip label={copied ? "Copied!" : "Copy log entry"} position="left" withArrow>
							<ActionIcon
								variant="subtle"
								color={copied ? "teal" : "gray"}
								size="xs"
								onClick={(e) => {
									e.stopPropagation();
									copy();
								}}
								className={classes.copyButton}
							>
								{copied ? <Check size={12} /> : <Copy size={12} />}
							</ActionIcon>
						</Tooltip>
					)}
				</CopyButton>
			</div>

			{hasDetails && (
				<Collapse in={expanded}>
					<div className={classes.details}>
						{entry.span && (
							<div className={classes.detailBlock}>
								<Text size="xs" c="dimmed" fw={600}>
									Span
								</Text>
								<Text size="xs" ff="monospace" c="dimmed">
									{entry.span.name} ({entry.span.target})
								</Text>
							</div>
						)}
						{entry.fields && (
							<div className={classes.detailBlock}>
								<Text size="xs" c="dimmed" fw={600}>
									Fields
								</Text>
								<div className={classes.fieldsContainer}>
									{Object.entries(entry.fields).map(([key, value]) => (
										<div key={key} className={classes.fieldRow}>
											<Text size="xs" ff="monospace" c="dimmed">
												{key}:
											</Text>
											<Text size="xs" ff="monospace" className={classes.fieldValue}>
												{formatFieldValue(value)}
											</Text>
										</div>
									))}
								</div>
							</div>
						)}
					</div>
				</Collapse>
			)}
		</div>
	);
}

export const LogEntry = memo(LogEntryComponent);
