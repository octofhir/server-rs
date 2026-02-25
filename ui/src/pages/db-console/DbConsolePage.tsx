import { useHotkeys, useLocalStorage } from "@octofhir/ui-kit";
import type * as monaco from "monaco-editor";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import {
	useSaveHistory,
	useSqlMutation,
	useQueryHistory,
} from "@/shared/api/hooks";
import { Badge, Group, Kbd, Text, Tooltip } from "@/shared/ui";
import type { SqlResponse } from "@/shared/api/types";
import { ExecutionStream } from "./components/ExecutionStream";
import { PromptEditor } from "./components/PromptEditor";
import { SchemaRail } from "./components/SchemaRail";
import { ActiveQueriesDropdown } from "./components/ActiveQueriesDropdown";
import type { StreamEntry } from "./components/StreamEntryCard";
import classes from "./DbConsolePage.module.css";

const INITIAL_QUERY = "SELECT * FROM patient LIMIT 10;";

// ─── Stream Reducer ───

type StreamAction =
	| { type: "add"; entry: StreamEntry }
	| {
			type: "update";
			id: string;
			result?: SqlResponse;
			error?: string;
			explainData?: SqlResponse;
			executionTimeMs?: number;
			status: "success" | "error";
	  }
	| { type: "update_explain"; id: string; explainData: SqlResponse }
	| { type: "toggle_expand"; id: string }
	| { type: "remove"; id: string }
	| { type: "clear" }
	| { type: "seed"; entries: StreamEntry[] };

function streamReducer(
	state: StreamEntry[],
	action: StreamAction,
): StreamEntry[] {
	switch (action.type) {
		case "add":
			return [...state, action.entry];
		case "update":
			return state.map((e) =>
				e.id === action.id
					? {
							...e,
							result: action.result,
							error: action.error,
							explainData: action.explainData ?? e.explainData,
							executionTimeMs: action.executionTimeMs,
							status: action.status,
						}
					: e,
			);
		case "update_explain":
			return state.map((e) =>
				e.id === action.id
					? { ...e, explainData: action.explainData }
					: e,
			);
		case "toggle_expand":
			return state.map((e) =>
				e.id === action.id ? { ...e, isExpanded: !e.isExpanded } : e,
			);
		case "remove":
			return state.filter((e) => e.id !== action.id);
		case "clear":
			return [];
		case "seed":
			// Prepend history but keep any live entries already in stream
			return state.length > 0
				? [...action.entries, ...state]
				: action.entries;
		default:
			return state;
	}
}

// ─── Page Component ───

export function DbConsolePage() {
	const queryRef = useRef(INITIAL_QUERY);
	const [stream, dispatch] = useReducer(streamReducer, []);
	const [railExpanded, setRailExpanded] = useLocalStorage({
		key: "db-console-rail-expanded",
		defaultValue: false,
	});
	const [searchFocusKey, setSearchFocusKey] = useState(0);
	const [historySeeded, setHistorySeeded] = useState(false);

	const [editorInstance, setEditorInstance] =
		useState<monaco.editor.IStandaloneCodeEditor | null>(null);
	const [modelInstance, setModelInstance] =
		useState<monaco.editor.ITextModel | null>(null);

	const sqlMutation = useSqlMutation();
	const explainMutation = useSqlMutation();
	const saveHistory = useSaveHistory();
	const { data: historyData } = useQueryHistory();

	// Seed stream with persisted history on first load
	useEffect(() => {
		if (historySeeded || !historyData?.entries) return;
		setHistorySeeded(true);

		const entries: StreamEntry[] = historyData.entries
			.slice()
			.reverse() // oldest first
			.map((h) => ({
				id: h.id,
				query: h.query,
				result: undefined,
				error: h.isError ? (h.errorMessage ?? "Query failed") : undefined,
				explainData: undefined,
				executionTimeMs: h.executionTimeMs ?? undefined,
				timestamp: new Date(h.createdAt),
				status: h.isError ? ("error" as const) : ("success" as const),
				isExpanded: false,
				fromHistory: true,
			}));

		if (entries.length > 0) {
			dispatch({ type: "seed", entries });
		}
	}, [historyData, historySeeded]);

	// ─── Handlers ───

	const toggleRail = useCallback(
		() => setRailExpanded((prev) => !prev),
		[setRailExpanded],
	);

	const handleQueryChange = useCallback((value: string) => {
		queryRef.current = value;
	}, []);

	const handleEditorMount = useCallback(
		(
			editor: monaco.editor.IStandaloneCodeEditor,
			model: monaco.editor.ITextModel,
		) => {
			setEditorInstance(editor);
			setModelInstance(model);
		},
		[],
	);

	const handleReplayQuery = useCallback(
		(query: string) => {
			if (editorInstance) {
				editorInstance.setValue(query);
				queryRef.current = query;
				editorInstance.focus();
			}
		},
		[editorInstance],
	);

	const handleToggleExpand = useCallback((id: string) => {
		dispatch({ type: "toggle_expand", id });
	}, []);

	const handleRemoveEntry = useCallback((id: string) => {
		dispatch({ type: "remove", id });
	}, []);

	const handleExecute = useCallback(
		(value?: string) => {
			if (sqlMutation.isPending) return; // prevent double-execution
			const queryToRun = value ?? queryRef.current;
			if (!queryToRun.trim()) return;
			queryRef.current = queryToRun;

			const entryId = crypto.randomUUID();

			// Add pending entry to stream
			dispatch({
				type: "add",
				entry: {
					id: entryId,
					query: queryToRun,
					timestamp: new Date(),
					status: "pending",
					isExpanded: true,
				},
			});

			// Execute SQL
			sqlMutation.mutate(
				{ query: queryToRun },
				{
					onSuccess: (data) => {
						dispatch({
							type: "update",
							id: entryId,
							result: data,
							executionTimeMs: data.executionTimeMs,
							status: "success",
						});
						saveHistory.mutate({
							query: queryToRun,
							executionTimeMs: data.executionTimeMs,
							rowCount: data.rowCount,
							isError: false,
						});
					},
					onError: (error) => {
						dispatch({
							type: "update",
							id: entryId,
							error: error.message,
							status: "error",
						});
						saveHistory.mutate({
							query: queryToRun,
							isError: true,
							errorMessage: error.message,
						});
					},
				},
			);

			// Auto EXPLAIN ANALYZE for SELECT queries only (WITH CTEs can contain DML)
			const trimmed = queryToRun.trim().toUpperCase();
			if (trimmed.startsWith("SELECT")) {
				explainMutation.mutate(
					{ query: `EXPLAIN ANALYZE ${queryToRun}` },
					{
						onSuccess: (data) => {
							dispatch({
								type: "update_explain",
								id: entryId,
								explainData: data,
							});
						},
					},
				);
			}

			// Clear editor after execution
			if (editorInstance) {
				editorInstance.setValue("");
				queryRef.current = "";
			}
		},
		[sqlMutation, explainMutation, saveHistory, editorInstance],
	);

	const handleClearStream = useCallback(() => {
		dispatch({ type: "clear" });
	}, []);

	const handleSearchFocus = useCallback(() => {
		setRailExpanded(true);
		setSearchFocusKey((k) => k + 1);
	}, [setRailExpanded]);

	// ─── Hotkeys ───

	useHotkeys([
		["mod+b", toggleRail],
		["mod+k", handleSearchFocus],
		["mod+l", handleClearStream],
	]);

	return (
		<div className={`${classes.container} page-enter`}>
			{/* Schema Rail */}
			<SchemaRail
				expanded={railExpanded}
				onToggle={toggleRail}
				onInsertQuery={handleReplayQuery}
				searchFocusKey={searchFocusKey}
			/>

			{/* Toolbar */}
			<div className={classes.toolbar}>
				<Group gap="sm">
					<Text size="sm" fw={700} style={{ letterSpacing: "-0.02em" }}>
						DB Console
					</Text>
					<Badge size="xs" variant="light" color="deep">
						readonly
					</Badge>
				</Group>
				<Group gap="sm">
					<Tooltip label="Search tables (Ctrl+K)">
						<Text
							size="xs"
							c="dimmed"
							style={{ cursor: "pointer" }}
							onClick={handleSearchFocus}
						>
							<Kbd size="xs">⌘K</Kbd>
						</Text>
					</Tooltip>
					<Tooltip label="Clear stream (Ctrl+L)">
						<Text
							size="xs"
							c="dimmed"
							style={{ cursor: "pointer" }}
							onClick={handleClearStream}
						>
							<Kbd size="xs">⌘L</Kbd>
						</Text>
					</Tooltip>
					<ActiveQueriesDropdown />
				</Group>
			</div>

			{/* Execution Stream */}
			<ExecutionStream
				entries={stream}
				onReplayQuery={handleReplayQuery}
				onToggleExpand={handleToggleExpand}
				onRemoveEntry={handleRemoveEntry}
			/>

			{/* Prompt Editor */}
			<div className={classes.editor}>
				<PromptEditor
					initialQuery={INITIAL_QUERY}
					onQueryChange={handleQueryChange}
					onExecute={handleExecute}
					onEditorMount={handleEditorMount}
					editorInstance={editorInstance}
					modelInstance={modelInstance}
					isPending={sqlMutation.isPending}
				/>
			</div>
		</div>
	);
}
