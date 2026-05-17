import { Box, Flex, Text, ThemeIcon } from "@/shared/ui";
import { useRef, useEffect, memo } from "react";

import { Pulse, CircleExclamation } from "@gravity-ui/icons";
import type { LogEntry as LogEntryType } from "@/shared/api/types";
import { LogEntry } from "./LogEntry";
import classes from "./LogStream.module.css";

interface LogStreamProps {
	logs: LogEntryType[];
	isConnected: boolean;
	isPaused: boolean;
	connectionError: string | null;
	autoScroll?: boolean;
}

function LogStreamComponent({
	logs,
	isConnected,
	isPaused,
	connectionError,
	autoScroll = true,
}: LogStreamProps) {
	const containerRef = useRef<HTMLDivElement>(null);
	const bottomRef = useRef<HTMLDivElement>(null);
	const userScrolledRef = useRef(false);
	const prevLogCountRef = useRef(logs.length);

	// Track if user has scrolled away from bottom
	useEffect(() => {
		const container = containerRef.current;
		if (!container) return;

		const handleScroll = () => {
			const { scrollTop, scrollHeight, clientHeight } = container;
			const isAtBottom = scrollHeight - scrollTop - clientHeight < 50;
			userScrolledRef.current = !isAtBottom;
		};

		container.addEventListener("scroll", handleScroll);
		return () => container.removeEventListener("scroll", handleScroll);
	}, []);

	// Auto-scroll to bottom when new logs arrive
	useEffect(() => {
		const hasNewLogs = logs.length > prevLogCountRef.current;
		prevLogCountRef.current = logs.length;

		if (hasNewLogs && autoScroll && !isPaused && !userScrolledRef.current && bottomRef.current) {
			bottomRef.current.scrollIntoView({ behavior: "smooth" });
		}
	}, [logs, autoScroll, isPaused]);

	if (connectionError) {
		return (
			<Flex justifyContent="center" alignItems="center" h="100%" className={classes.emptyState}>
				<Flex direction="column" align="center" gap="md">
					<ThemeIcon variant="light" color="fire" size={64} radius="xl">
						<CircleExclamation size={32} />
					</ThemeIcon>
					<Text size="lg" fw={600}>
						Connection Error
					</Text>
					<Text size="sm" c="dimmed" ta="center" maw={400}>
						{connectionError}
					</Text>
				</Flex>
			</Flex>
		);
	}

	if (!isConnected) {
		return (
			<Flex justifyContent="center" alignItems="center" h="100%" className={classes.emptyState}>
				<Flex direction="column" align="center" gap="md">
					<ThemeIcon variant="light" color="gray" size={64} radius="xl">
						<Pulse size={32} />
					</ThemeIcon>
					<Text size="lg" fw={600}>
						Connecting...
					</Text>
					<Text size="sm" c="dimmed">
						Establishing connection to log stream
					</Text>
				</Flex>
			</Flex>
		);
	}

	if (logs.length === 0) {
		return (
			<Flex justifyContent="center" alignItems="center" h="100%" className={classes.emptyState}>
				<Flex direction="column" align="center" gap="md">
					<ThemeIcon variant="light" color="gray" size={64} radius="xl">
						<Pulse size={32} />
					</ThemeIcon>
					<Text size="lg" fw={600}>
						No Logs
					</Text>
					<Text size="sm" c="dimmed" ta="center" maw={400}>
						{isPaused
							? "Stream is paused. Resume to see new logs."
							: "Waiting for log events. Logs will appear here as they are generated."}
					</Text>
				</Flex>
			</Flex>
		);
	}

	return (
		<Box ref={containerRef} className={classes.container}>
			<div className={classes.logList}>
				{logs.map((log) => (
					<LogEntry key={log.id} entry={log} />
				))}
				<div ref={bottomRef} className={classes.bottomAnchor} />
			</div>
		</Box>
	);
}

export const LogStream = memo(LogStreamComponent);
