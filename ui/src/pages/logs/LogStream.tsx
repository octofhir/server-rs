import { EmptyState, Skeleton } from "@octofhir/ui-kit";
import { memo, useEffect, useRef } from "react";

import { CircleExclamation, Pulse } from "@gravity-ui/icons";
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

function ConnectingSkeleton() {
	return (
		<output className={classes.skeletonList} aria-busy="true" aria-label="Connecting to log stream">
			{Array.from({ length: 8 }).map((_, i) => (
				// biome-ignore lint/suspicious/noArrayIndexKey: static placeholder rows
				<div key={i} className={classes.skeletonRow}>
					<Skeleton className={classes.skeletonTimestamp} />
					<Skeleton className={classes.skeletonBadge} />
					<Skeleton className={classes.skeletonMessage} />
				</div>
			))}
		</output>
	);
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
			<div className={classes.emptyState}>
				<EmptyState
					image={<CircleExclamation width={48} height={48} className={classes.errorIcon} aria-hidden="true" />}
					title="Connection error"
					description={connectionError}
				/>
			</div>
		);
	}

	if (!isConnected) {
		return (
			<div ref={containerRef} className={classes.container}>
				<ConnectingSkeleton />
			</div>
		);
	}

	if (logs.length === 0) {
		return (
			<div className={classes.emptyState}>
				<EmptyState
					image={<Pulse width={48} height={48} className={classes.emptyIcon} aria-hidden="true" />}
					title={isPaused ? "Stream paused" : "No logs yet"}
					description={
						isPaused
							? "The stream is paused. Resume to see new log events."
							: "Waiting for log events. Logs will appear here as they are generated."
					}
				/>
			</div>
		);
	}

	return (
		<div ref={containerRef} className={classes.container}>
			<div className={classes.logList}>
				{logs.map((log) => (
					<LogEntry key={log.id} entry={log} />
				))}
				<div ref={bottomRef} className={classes.bottomAnchor} />
			</div>
		</div>
	);
}

export const LogStream = memo(LogStreamComponent);
