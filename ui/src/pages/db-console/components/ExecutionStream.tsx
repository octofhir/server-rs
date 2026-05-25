import { Kbd, Text, UnstyledButton } from "@octofhir/ui-kit";
import { useEffect, useMemo, useRef, useState } from "react";
import { ClockArrowRotateLeft, Terminal, ChevronRight, ChevronDown } from "@gravity-ui/icons";
import { StreamEntryCard, type StreamEntry } from "./StreamEntryCard";
import classes from "../DbConsolePage.module.css";

interface ExecutionStreamProps {
	entries: StreamEntry[];
	onReplayQuery: (query: string) => void;
	onToggleExpand: (id: string) => void;
	onRemoveEntry: (id: string) => void;
}

export function ExecutionStream({
	entries,
	onReplayQuery,
	onToggleExpand,
	onRemoveEntry,
}: ExecutionStreamProps) {
	const streamEndRef = useRef<HTMLDivElement>(null);
	const prevScrollKeyRef = useRef("");
	const [historyOpen, setHistoryOpen] = useState(false);

	const { historyEntries, sessionEntries } = useMemo(() => {
		const history: StreamEntry[] = [];
		const session: StreamEntry[] = [];
		for (const e of entries) {
			if (e.fromHistory) {
				history.push(e);
			} else {
				session.push(e);
			}
		}
		return { historyEntries: history, sessionEntries: session };
	}, [entries]);

	// Auto-scroll when new session entries added or last entry status changes
	useEffect(() => {
		const last = sessionEntries[sessionEntries.length - 1];
		const key = `${sessionEntries.length}-${last?.status}`;
		if (key !== prevScrollKeyRef.current) {
			prevScrollKeyRef.current = key;
			streamEndRef.current?.scrollIntoView({ behavior: "smooth" });
		}
	});

	const hasHistory = historyEntries.length > 0;
	const hasSession = sessionEntries.length > 0;

	if (!hasHistory && !hasSession) {
		return (
			<div className={classes.stream}>
				<div className={classes.emptyState}>
					<Terminal size={40} stroke={1.2} />
					<Text size="sm" fw={500}>
						Run your first query
					</Text>
					<Text size="xs" c="dimmed">
						Press <Kbd size="xs">Ctrl</Kbd> + <Kbd size="xs">Enter</Kbd> to
						execute
					</Text>
				</div>
			</div>
		);
	}

	return (
		<div className={classes.stream}>
			{/* History section */}
			{hasHistory && (
				<>
					<UnstyledButton
						className={classes.historySectionToggle}
						onClick={() => setHistoryOpen((v) => !v)}
					>
						<div className={classes.historyToggleContent}>
							{historyOpen ? (
								<ChevronDown size={14} />
							) : (
								<ChevronRight size={14} />
							)}
							<ClockArrowRotateLeft size={14} />
							<Text size="xs" fw={500}>
								Previous sessions
							</Text>
							<Text size="xs" c="dimmed">
								({historyEntries.length})
							</Text>
						</div>
					</UnstyledButton>

					{historyOpen &&
						historyEntries.map((entry) => (
							<StreamEntryCard
								key={entry.id}
								entry={entry}
								onReplayQuery={onReplayQuery}
								onToggleExpand={onToggleExpand}
								onRemoveEntry={onRemoveEntry}
							/>
						))}

					{/* Separator between history and session */}
					{hasSession && <div className={classes.streamSeparator} />}
				</>
			)}

			{/* Current session entries */}
			{sessionEntries.map((entry) => (
				<StreamEntryCard
					key={entry.id}
					entry={entry}
					onReplayQuery={onReplayQuery}
					onToggleExpand={onToggleExpand}
					onRemoveEntry={onRemoveEntry}
				/>
			))}
			<div ref={streamEndRef} />
		</div>
	);
}
