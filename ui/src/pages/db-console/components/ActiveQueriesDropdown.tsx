import { useCallback, useState } from "react";
import {
	ActionIcon,
	Badge,
	Box,
	Group,
	Popover,
	ScrollArea,
	Text,
	Tooltip,
	UnstyledButton,
} from "@/shared/ui";
import { IconActivity, IconPlayerStop } from "@tabler/icons-react";
import { modals, notifications, useDisclosure } from "@octofhir/ui-kit";
import { useActiveQueries, useTerminateQuery } from "@/shared/api/hooks";
import type { ActiveQuery } from "@/shared/api/types";

function formatDuration(ms?: number): string {
	if (ms == null) return "-";
	if (ms < 1000) return `${ms}ms`;
	if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
	return `${(ms / 60000).toFixed(1)}m`;
}

function stateColor(state?: string): string {
	switch (state) {
		case "active":
			return "primary";
		case "idle":
			return "deep";
		case "idle in transaction":
			return "warm";
		case "idle in transaction (aborted)":
			return "fire";
		default:
			return "gray";
	}
}

function QueryItem({
	query,
	onTerminate,
	isTerminating,
}: {
	query: ActiveQuery;
	onTerminate: (pid: number) => void;
	isTerminating: boolean;
}) {
	return (
		<Box
			p="xs"
			style={{ borderBottom: "1px solid var(--octo-border-subtle)" }}
		>
			<Group justify="space-between" wrap="nowrap" mb={4}>
				<Group gap={6} wrap="nowrap">
					<Badge size="xs" variant="light" color={stateColor(query.state)}>
						{query.state ?? "unknown"}
					</Badge>
					<Text size="xs" c="dimmed">
						PID {query.pid}
					</Text>
				</Group>
				<Group gap={4} wrap="nowrap">
					<Text size="xs" c="dimmed">
						{formatDuration(query.durationMs)}
					</Text>
					<Tooltip label="Terminate query">
						<ActionIcon
							variant="subtle"
							size="xs"
							color="fire"
							onClick={() => onTerminate(query.pid)}
							loading={isTerminating}
						>
							<IconPlayerStop size={12} />
						</ActionIcon>
					</Tooltip>
				</Group>
			</Group>
			{query.query && (
				<Text
					size="xs"
					ff="monospace"
					lineClamp={2}
					style={{ wordBreak: "break-all" }}
				>
					{query.query}
				</Text>
			)}
		</Box>
	);
}

export function ActiveQueriesDropdown() {
	const [opened, { toggle, close }] = useDisclosure(false);
	const { data, isLoading } = useActiveQueries(true);
	const terminateMutation = useTerminateQuery();
	const [terminatingPid, setTerminatingPid] = useState<number | null>(null);

	const queries = data?.queries ?? [];
	const activeCount = queries.filter((q) => q.state === "active").length;

	const handleTerminate = useCallback(
		(pid: number) => {
			modals.openConfirmModal({
				title: "Terminate Query",
				children: (
					<Text size="sm">
						Terminate query with PID <strong>{pid}</strong>?
					</Text>
				),
				labels: { confirm: "Terminate", cancel: "Cancel" },
				confirmProps: { color: "red" },
				onConfirm: () => {
					setTerminatingPid(pid);
					terminateMutation.mutate(
						{ pid },
						{
							onSuccess: (res) => {
								notifications.show({
									title: res.terminated
										? "Query terminated"
										: "Termination sent",
									message: `Signal sent to PID ${pid}`,
									color: "green",
								});
							},
							onError: (err) => {
								notifications.show({
									title: "Failed to terminate",
									message: err.message,
									color: "red",
								});
							},
							onSettled: () => setTerminatingPid(null),
						},
					);
				},
			});
		},
		[terminateMutation],
	);

	return (
		<Popover
			opened={opened}
			onClose={close}
			position="bottom-end"
			width={380}
			shadow="md"
		>
			<Popover.Target>
				<Tooltip label="Active queries">
					<UnstyledButton
						onClick={toggle}
						style={{
							display: "inline-flex",
							alignItems: "center",
							gap: 4,
							padding: "2px 6px",
							borderRadius: "var(--mantine-radius-sm)",
						}}
					>
						<IconActivity
							size={15}
							style={{ opacity: queries.length > 0 ? 1 : 0.5 }}
						/>
						{queries.length > 0 && (
							<Text
								size="xs"
								fw={600}
								c={activeCount > 0 ? "primary" : "dimmed"}
							>
								{queries.length}
							</Text>
						)}
					</UnstyledButton>
				</Tooltip>
			</Popover.Target>
			<Popover.Dropdown p={0}>
				<Group
					justify="space-between"
					px="sm"
					py="xs"
					style={{
						borderBottom: "1px solid var(--octo-border-subtle)",
					}}
				>
					<Text size="xs" fw={600}>
						Active Queries
					</Text>
					{queries.length > 0 && (
						<Badge size="xs" variant="light">
							{queries.length}
						</Badge>
					)}
				</Group>
				<ScrollArea.Autosize mah={300}>
					{isLoading && (
						<Text size="xs" c="dimmed" ta="center" py="md">
							Loading...
						</Text>
					)}
					{!isLoading && queries.length === 0 && (
						<Text size="xs" c="dimmed" ta="center" py="xl">
							No active queries
						</Text>
					)}
					{queries.map((q) => (
						<QueryItem
							key={q.pid}
							query={q}
							onTerminate={handleTerminate}
							isTerminating={terminatingPid === q.pid}
						/>
					))}
				</ScrollArea.Autosize>
			</Popover.Dropdown>
		</Popover>
	);
}
