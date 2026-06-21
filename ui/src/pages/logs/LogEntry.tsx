import { ActionIcon, Badge, ClipboardButton, Collapse, Text, Tooltip } from "@octofhir/ui-kit";
import { memo, useState } from "react";

import { ChevronDown, ChevronRight } from "lucide-react";
import type { LogEntry as LogEntryType, LogLevel } from "@/shared/api/types";
import classes from "./LogEntry.module.css";

interface LogEntryProps {
	entry: LogEntryType;
}

type BadgeColor = "neutral" | "primary" | "warning" | "danger";

const LEVEL_COLORS: Record<LogLevel, BadgeColor> = {
	trace: "neutral",
	debug: "primary",
	info: "primary",
	warn: "warning",
	error: "danger",
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
				{...(hasDetails
					? {
							role: "button",
							tabIndex: 0,
							"aria-expanded": expanded,
							onClick: () => setExpanded(!expanded),
							onKeyDown: (e: React.KeyboardEvent) => {
								if (e.key === "Enter" || e.key === " ") {
									e.preventDefault();
									setExpanded(!expanded);
								}
							},
						}
					: {})}
			>
				{hasDetails ? (
					<ActionIcon
						variant="subtle"
						size="sm"
						className={classes.expandIcon}
						aria-label={expanded ? "Collapse log entry" : "Expand log entry"}
						tabIndex={-1}
						onClick={(e) => {
							e.stopPropagation();
							setExpanded(!expanded);
						}}
					>
						{expanded ? (
							<ChevronDown width={12} height={12} aria-hidden="true" />
						) : (
							<ChevronRight width={12} height={12} aria-hidden="true" />
						)}
					</ActionIcon>
				) : (
					<span className={classes.expandSpacer} />
				)}

				<Tooltip content={formatFullTimestamp(entry.timestamp)} placement="top">
					<Text variant="caption-2" color="secondary" className={`${classes.timestamp} ${classes.mono}`}>
						{formatTimestamp(entry.timestamp)}
					</Text>
				</Tooltip>

				<Badge
					size="xs"
					color={LEVEL_COLORS[entry.level]}
					className={classes.levelBadge}
				>
					{entry.level.toUpperCase()}
				</Badge>

				<Text
					variant="caption-2"
					color="secondary"
					ellipsis
					className={`${classes.target} ${classes.mono}`}
				>
					{entry.target}
				</Text>

				<Text variant="body-2" ellipsis className={`${classes.message} ${classes.mono}`}>
					{entry.message}
				</Text>

				<ClipboardButton
					text={copyContent}
					variant="subtle"
					size="sm"
					tooltipInitialText="Copy log entry"
					tooltipSuccessText="Copied!"
					className={classes.copyButton}
					onClick={(e) => e.stopPropagation()}
				/>
			</div>

			{hasDetails && (
				<Collapse in={expanded}>
					<div className={classes.details}>
						{entry.span && (
							<div className={classes.detailBlock}>
								<Text variant="caption-2" color="secondary">
									<strong>Span</strong>
								</Text>
								<Text variant="caption-2" color="secondary" className={classes.mono}>
									{entry.span.name} ({entry.span.target})
								</Text>
							</div>
						)}
						{entry.fields && (
							<div className={classes.detailBlock}>
								<Text variant="caption-2" color="secondary">
									<strong>Fields</strong>
								</Text>
								<div className={classes.fieldsContainer}>
									{Object.entries(entry.fields).map(([key, value]) => (
										<div key={key} className={classes.fieldRow}>
											<Text variant="caption-2" color="secondary" className={classes.mono}>
												{key}:
											</Text>
											<Text
												variant="caption-2"
												className={`${classes.fieldValue} ${classes.mono}`}
											>
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
