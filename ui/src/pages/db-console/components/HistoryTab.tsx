import { ActionIcon, Badge, ScrollArea, Text, Tooltip, modals } from "@octofhir/ui-kit";
import { useCallback } from "react";
import { TrashBin, Clock } from "@gravity-ui/icons";
import { useQueryHistory, useClearHistory } from "@/shared/api/hooks";
import type { QueryHistoryEntry } from "@/shared/api/types";
import classes from "../DbConsolePage.module.css";

interface HistoryTabProps {
	onSelectQuery: (query: string) => void;
}

function formatTimeAgo(dateStr: string): string {
	const date = new Date(dateStr);
	const now = new Date();
	const diff = now.getTime() - date.getTime();
	const mins = Math.floor(diff / 60000);
	if (mins < 1) return "just now";
	if (mins < 60) return `${mins}m ago`;
	const hours = Math.floor(mins / 60);
	if (hours < 24) return `${hours}h ago`;
	const days = Math.floor(hours / 24);
	return `${days}d ago`;
}

function HistoryItem({
	entry,
	onSelect,
}: { entry: QueryHistoryEntry; onSelect: (query: string) => void }) {
	return (
		<div
			onClick={() => onSelect(entry.query)}
			className={classes.historyItem}
		>
			<Text
				size="xs"
				ff="monospace"
				className={classes.queryClamp2}
			>
				{entry.query}
			</Text>
			<div className={classes.historyMeta}>
				<Text size="xs" c="dimmed">
					{formatTimeAgo(entry.createdAt)}
				</Text>
				{entry.executionTimeMs != null && (
					<Text size="xs" c="dimmed">
						{entry.executionTimeMs}ms
					</Text>
				)}
				{entry.isError && (
					<Badge size="xs" color="fire" variant="light">
						error
					</Badge>
				)}
				{entry.rowCount != null && !entry.isError && (
					<Text size="xs" c="dimmed">
						{entry.rowCount} rows
					</Text>
				)}
			</div>
		</div>
	);
}

export function HistoryTab({ onSelectQuery }: HistoryTabProps) {
	const { data, isLoading } = useQueryHistory();
	const clearMutation = useClearHistory();

	const handleClear = useCallback(() => {
		modals.openConfirmModal({
			title: "Clear History",
			children: (
				<Text size="sm">Are you sure you want to clear all query history?</Text>
			),
			labels: { confirm: "Clear", cancel: "Cancel" },
			confirmProps: { color: "red" },
			onConfirm: () => clearMutation.mutate(),
		});
	}, [clearMutation]);

	const entries = data?.entries ?? [];

	return (
		<div className={classes.sideTabRoot}>
			<div className={classes.sideTabHeader}>
				<div className={classes.sideTabHeaderTitle}>
					<Clock size={14} className={classes.mutedIcon} />
					<Text size="xs" fw={500} c="dimmed">
						History
					</Text>
				</div>
				{entries.length > 0 && (
					<Tooltip label="Clear history">
						<ActionIcon
							variant="subtle"
							size="xs"
							color="fire"
							onClick={handleClear}
							loading={clearMutation.isPending}
						>
							<TrashBin size={12} />
						</ActionIcon>
					</Tooltip>
				)}
			</div>
			<ScrollArea className={classes.sideTabScroll}>
				{isLoading && (
					<Text size="xs" c="dimmed" ta="center" py="md">
						Loading...
					</Text>
				)}
				{!isLoading && entries.length === 0 && (
					<Text size="xs" c="dimmed" ta="center" py="xl">
						No queries yet
					</Text>
				)}
				{entries.map((entry) => (
					<HistoryItem key={entry.id} entry={entry} onSelect={onSelectQuery} />
				))}
			</ScrollArea>
		</div>
	);
}
