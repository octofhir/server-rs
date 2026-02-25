import { useCallback } from "react";
import { Stack, Text, Group, Badge, ActionIcon, ScrollArea, Box, Tooltip } from "@/shared/ui";
import { IconTrash, IconClock } from "@tabler/icons-react";
import { useQueryHistory, useClearHistory } from "@/shared/api/hooks";
import { modals } from "@octofhir/ui-kit";
import type { QueryHistoryEntry } from "@/shared/api/types";

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
		<Box
			onClick={() => onSelect(entry.query)}
			style={{
				padding: "8px 12px",
				cursor: "pointer",
				borderBottom: "1px solid var(--octo-border-subtle)",
			}}
			onMouseEnter={(e) => {
				(e.currentTarget as HTMLElement).style.backgroundColor = "var(--octo-surface-2)";
			}}
			onMouseLeave={(e) => {
				(e.currentTarget as HTMLElement).style.backgroundColor = "transparent";
			}}
		>
			<Text
				size="xs"
				ff="monospace"
				lineClamp={2}
				style={{ wordBreak: "break-all" }}
			>
				{entry.query}
			</Text>
			<Group gap={6} mt={4}>
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
			</Group>
		</Box>
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
		<Stack gap={0} h="100%">
			<Group justify="space-between" px="sm" py="xs" style={{ flexShrink: 0 }}>
				<Group gap={4}>
					<IconClock size={14} style={{ opacity: 0.5 }} />
					<Text size="xs" fw={500} c="dimmed">
						History
					</Text>
				</Group>
				{entries.length > 0 && (
					<Tooltip label="Clear history">
						<ActionIcon
							variant="subtle"
							size="xs"
							color="fire"
							onClick={handleClear}
							loading={clearMutation.isPending}
						>
							<IconTrash size={12} />
						</ActionIcon>
					</Tooltip>
				)}
			</Group>
			<ScrollArea style={{ flex: 1 }}>
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
		</Stack>
	);
}
