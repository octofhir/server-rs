import { modals, notifications, useDisclosure } from "@octofhir/ui-kit";
import { useCallback, useState } from "react";
import {
	ActionIcon,
	Badge,
	Popover,
	ScrollArea,
	Text,
	Tooltip,
	UnstyledButton,
} from "@octofhir/ui-kit";
import { Pulse, Stop } from "@gravity-ui/icons";
import { useActiveQueries, useTerminateQuery } from "@/shared/api/hooks";
import type { ActiveQuery } from "@/shared/api/types";
import classes from "../DbConsolePage.module.css";

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
		<div className={classes.queryItem}>
			<div className={classes.queryItemHeader}>
				<div className={classes.queryItemMeta}>
					<Badge size="xs" variant="light" color={stateColor(query.state)}>
						{query.state ?? "unknown"}
					</Badge>
					<Text size="xs" c="dimmed">
						PID {query.pid}
					</Text>
				</div>
				<div className={classes.queryItemMeta}>
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
							<Stop size={12} />
						</ActionIcon>
					</Tooltip>
				</div>
			</div>
			{query.query && (
				<Text
					size="xs"
					ff="monospace"
					className={classes.queryClamp2}
				>
					{query.query}
				</Text>
			)}
		</div>
	);
}

export function ActiveQueriesDropdown() {
	const [opened, { open, close }] = useDisclosure(false);
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
			open={opened}
			onOpenChange={(nextOpen) => (nextOpen ? open() : close())}
			placement="bottom-end"
			trigger="click"
			content={
				<div className={classes.activeQueriesPopover}>
					<div className={classes.activeQueriesHeader}>
						<Text size="xs" fw={600}>
							Active Queries
						</Text>
						{queries.length > 0 && (
							<Badge size="xs" variant="light">
								{queries.length}
							</Badge>
						)}
					</div>
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
				</div>
			}
		>
			<Tooltip label="Active queries">
				<UnstyledButton
					className={classes.activeQueriesTrigger}
				>
					<Pulse
						size={15}
						className={queries.length > 0 ? undefined : classes.mutedIcon}
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
		</Popover>
	);
}
