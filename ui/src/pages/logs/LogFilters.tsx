import { ActionIcon, Badge, Button, Flex, Menu, TextInput, Tooltip } from "@/shared/ui";
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
			<Flex gap="3" wrap="wrap" alignItems="center" className={classes.toolbar}>
				<Flex gap="2" wrap="wrap" alignItems="center" className={classes.primaryControls}>
					<TextInput
						placeholder="Search logs..."
						leftSection={<Magnifier size={14} />}
						value={filters.search || ""}
						onUpdate={handleSearchChange}
						className={classes.searchInput}
					/>

					<Menu>
						<Menu.Trigger>
							<Button
								view="flat-secondary"
							>
								<Button.Icon><FunnelXmark size={14} /></Button.Icon>
								Levels
								{!allLevelsSelected && (
									<Badge theme="info" style={{ marginLeft: "var(--g-spacing-1)" }}>
										{filters.levels.length}
									</Badge>
								)}
							</Button>
						</Menu.Trigger>
						<Menu.Content>
							{LOG_LEVELS.map((level) => (
								<Menu.Item
									key={level.value}
									onClick={() => handleLevelToggle(level.value)}
									selected={filters.levels.includes(level.value)}
								>
									{level.label}
								</Menu.Item>
							))}
							<Menu.Divider />
							<Menu.Item
								onClick={() =>
									onFiltersChange({
										levels: allLevelsSelected ? [] : LOG_LEVELS.map((l) => l.value),
									})
								}
							>
								{allLevelsSelected ? "Deselect All" : "Select All"}
							</Menu.Item>
						</Menu.Content>
					</Menu>

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
				</Flex>

				<Flex gap="2" className={classes.actions}>
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

					<Menu>
						<Menu.Trigger>
							<Tooltip content="Export logs">
								<ActionIcon view="flat-secondary" aria-label="Export logs">
									<ArrowDownToLine size={18} />
								</ActionIcon>
							</Tooltip>
						</Menu.Trigger>
						<Menu.Content>
							<Menu.Item
								onClick={() => onExport("json")}
							>
								<Menu.Icon><CurlyBrackets size={14} /></Menu.Icon>
								JSON
							</Menu.Item>
							<Menu.Item
								onClick={() => onExport("text")}
							>
								<Menu.Icon><FileText size={14} /></Menu.Icon>
								Plain Text
							</Menu.Item>
						</Menu.Content>
					</Menu>
				</Flex>
			</Flex>
		</div>
	);
}

export const LogFilters = memo(LogFiltersComponent);
