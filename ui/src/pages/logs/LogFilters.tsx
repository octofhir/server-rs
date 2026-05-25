import { DropdownMenu } from "@gravity-ui/uikit";
import { ActionIcon, Badge, Button, TextInput, Tooltip } from "@octofhir/ui-kit";
import { memo } from "react";

import {
	Magnifier,
	FunnelXmark,
	ArrowDownToLine,
	Pause,
	Play,
	TrashBin,
	CurlyBrackets,
	FileText,
} from "@gravity-ui/icons";
import type { LogFilters as LogFiltersType, LogLevel } from "@/shared/api/types";
import classes from "./LogFilters.module.css";

interface LogFiltersProps {
	filters: LogFiltersType;
	isPaused: boolean;
	logCount: number;
	pendingCount?: number;
	onFiltersChange: (filters: Partial<LogFiltersType>) => void;
	onPause: () => void;
	onResume: () => void;
	onClear: () => void;
	onExport: (format: "json" | "text") => void;
}

const LOG_LEVELS: { value: LogLevel; label: string; color: string }[] = [
	{ value: "trace", label: "TRACE", color: "gray" },
	{ value: "debug", label: "DEBUG", color: "primary" },
	{ value: "info", label: "INFO", color: "primary" },
	{ value: "warn", label: "WARN", color: "warm" },
	{ value: "error", label: "ERROR", color: "fire" },
];

function LogFiltersComponent({
	filters,
	isPaused,
	logCount,
	pendingCount = 0,
	onFiltersChange,
	onPause,
	onResume,
	onClear,
	onExport,
}: LogFiltersProps) {
	const handleLevelToggle = (level: LogLevel) => {
		const newLevels = filters.levels.includes(level)
			? filters.levels.filter((l) => l !== level)
			: [...filters.levels, level];
		onFiltersChange({ levels: newLevels });
	};

	const handleSearchChange = (value: string) => {
		onFiltersChange({ search: value || undefined });
	};

	const allLevelsSelected = filters.levels.length === LOG_LEVELS.length;
	const noLevelsSelected = filters.levels.length === 0;

	return (
		<div className={classes.container}>
			<div className={classes.toolbar}>
				<div className={classes.primaryControls}>
					<TextInput
						placeholder="Search logs..."
						leftSection={<Magnifier size={14} />}
						value={filters.search || ""}
						onUpdate={handleSearchChange}
						className={classes.searchInput}
					/>

					<DropdownMenu
						size="s"
						popupProps={{ placement: "bottom-start" }}
						renderSwitcher={(switcherProps) => (
							<Button
								{...switcherProps}
								view="flat-secondary"
								leftSection={<FunnelXmark size={14} />}
								className={classes.filterButton}
							>
								Levels
								{!allLevelsSelected && (
									<Badge theme="info" className={classes.levelCount}>
										{filters.levels.length}
									</Badge>
								)}
							</Button>
						)}
						items={[
							...LOG_LEVELS.map((level) => ({
								text: level.label,
								action: () => handleLevelToggle(level.value),
								selected: filters.levels.includes(level.value),
							})),
							[
								{
									text: allLevelsSelected ? "Deselect All" : "Select All",
									action: () =>
										onFiltersChange({
											levels: allLevelsSelected ? [] : LOG_LEVELS.map((l) => l.value),
										}),
								},
							],
						]}
					/>

					<Badge
						theme={noLevelsSelected ? "normal" : "info"}
					>
						{logCount} logs
					</Badge>

					{isPaused && pendingCount > 0 && (
						<Badge theme="warning">
							+{pendingCount} pending
						</Badge>
					)}
				</div>

				<div className={classes.actions}>
					<Tooltip content={isPaused ? "Resume stream" : "Pause stream"}>
						<ActionIcon
							view={isPaused ? "action" : "flat-secondary"}
							aria-label={isPaused ? "Resume stream" : "Pause stream"}
							onClick={isPaused ? onResume : onPause}
						>
							{isPaused ? <Play size={18} /> : <Pause size={18} />}
						</ActionIcon>
					</Tooltip>

					<Tooltip content="Clear logs">
						<ActionIcon view="flat-secondary" aria-label="Clear logs" onClick={onClear}>
							<TrashBin size={18} />
						</ActionIcon>
					</Tooltip>

					<DropdownMenu
						size="s"
						popupProps={{ placement: "bottom-end" }}
						renderSwitcher={(switcherProps) => (
							<Button
								{...switcherProps}
								view="flat-secondary"
								leftSection={<ArrowDownToLine size={14} />}
								className={classes.exportButton}
							>
								Export
							</Button>
						)}
						items={[
							{
								text: "JSON",
								iconStart: <CurlyBrackets size={14} />,
								action: () => onExport("json"),
							},
							{
								text: "Plain Text",
								iconStart: <FileText size={14} />,
								action: () => onExport("text"),
							},
						]}
					/>
				</div>
			</div>
		</div>
	);
}

export const LogFilters = memo(LogFiltersComponent);
