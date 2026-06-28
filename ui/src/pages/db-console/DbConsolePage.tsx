import {
  Badge,
  Kbd,
  Resizable,
  Text,
  Tooltip,
  useHotkeys,
  useLocalStorage,
} from "@octofhir/ui-kit";
import { History as HistoryIcon } from "lucide-react";
import type * as monaco from "monaco-editor";
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { applyResultLimit, formatSqlError, parseTimeoutMs } from "@/entities/db-query";
import { useQueryHistory, useSaveHistory, useSqlMutation } from "@/shared/api/hooks";
import type { SqlResponse } from "@/shared/api/types";
import { ActiveQueriesDropdown } from "./components/ActiveQueriesDropdown";
import { HistoryDrawer } from "./components/HistoryDrawer";
import { QueryEditor } from "./components/QueryEditor";
import { ResultPanel } from "./components/ResultPanel";
import classes from "./DbConsolePage.module.css";
import type { StreamEntry } from "./types";

const INITIAL_QUERY = "SELECT * FROM patient LIMIT 10;";
const DEFAULT_RESULT_LIMIT = "200";
const DEFAULT_SQL_TIMEOUT = "120000";

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
  | { type: "remove"; id: string }
  | { type: "clear" }
  | { type: "seed"; entries: StreamEntry[] };

function streamReducer(state: StreamEntry[], action: StreamAction): StreamEntry[] {
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
          : e
      );
    case "update_explain":
      return state.map((e) => (e.id === action.id ? { ...e, explainData: action.explainData } : e));
    case "remove":
      return state.filter((e) => e.id !== action.id);
    case "clear":
      return [];
    case "seed":
      return state.length > 0 ? [...action.entries, ...state] : action.entries;
    default:
      return state;
  }
}

// ─── Page Component ───

export function DbConsolePage() {
  const queryRef = useRef(INITIAL_QUERY);
  const [stream, dispatch] = useReducer(streamReducer, []);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [historySeeded, setHistorySeeded] = useState(false);

  const [resultLimit, setResultLimit] = useLocalStorage({
    key: "db-console-result-limit",
    defaultValue: DEFAULT_RESULT_LIMIT,
    validate: (value): value is string => typeof value === "string",
  });
  const [sqlTimeout, setSqlTimeout] = useLocalStorage({
    key: "db-console-sql-timeout",
    defaultValue: DEFAULT_SQL_TIMEOUT,
    validate: (value): value is string => typeof value === "string",
  });

  const [editorInstance, setEditorInstance] = useState<monaco.editor.IStandaloneCodeEditor | null>(
    null
  );
  const [modelInstance, setModelInstance] = useState<monaco.editor.ITextModel | null>(null);

  const sqlMutation = useSqlMutation();
  const explainMutation = useSqlMutation();
  const saveHistory = useSaveHistory();
  const { data: historyData } = useQueryHistory();

  // Seed history (for the drawer) on first load.
  useEffect(() => {
    if (historySeeded || !historyData?.entries) return;
    setHistorySeeded(true);

    const entries: StreamEntry[] = historyData.entries
      .slice()
      .reverse()
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

  // Active entry shown in the result panel: explicit selection, else newest live entry.
  const activeEntry = useMemo(() => {
    if (activeId) {
      const found = stream.find((e) => e.id === activeId);
      if (found) return found;
    }
    for (let i = stream.length - 1; i >= 0; i--) {
      if (!stream[i].fromHistory) return stream[i];
    }
    return undefined;
  }, [stream, activeId]);

  // ─── Handlers ───

  const handleQueryChange = useCallback((value: string) => {
    queryRef.current = value;
  }, []);

  const handleEditorMount = useCallback(
    (editor: monaco.editor.IStandaloneCodeEditor, model: monaco.editor.ITextModel) => {
      setEditorInstance(editor);
      setModelInstance(model);
    },
    []
  );

  const loadIntoEditor = useCallback(
    (query: string) => {
      if (editorInstance) {
        editorInstance.setValue(query);
        editorInstance.focus();
      }
      queryRef.current = query;
    },
    [editorInstance]
  );

  const handleSelectHistory = useCallback(
    (entry: StreamEntry) => {
      loadIntoEditor(entry.query);
      setActiveId(entry.id);
    },
    [loadIntoEditor]
  );

  const handleExecute = useCallback(
    (value?: string) => {
      if (sqlMutation.isPending) return;
      const sourceQuery = value ?? queryRef.current;
      if (!sourceQuery.trim()) return;
      const queryToRun = applyResultLimit(sourceQuery, resultLimit);
      const timeoutMs = parseTimeoutMs(sqlTimeout);
      queryRef.current = sourceQuery;

      const entryId = crypto.randomUUID();
      setActiveId(entryId);

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

      sqlMutation.mutate(
        { query: queryToRun, timeoutMs },
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
            const errorMessage = formatSqlError(error);
            dispatch({ type: "update", id: entryId, error: errorMessage, status: "error" });
            saveHistory.mutate({ query: queryToRun, isError: true, errorMessage });
          },
        }
      );

      // Auto EXPLAIN ANALYZE for plain SELECT statements.
      const trimmed = queryToRun.trim().toUpperCase();
      if (trimmed.startsWith("SELECT")) {
        explainMutation.mutate(
          { query: `EXPLAIN (ANALYZE, FORMAT JSON) ${queryToRun}`, timeoutMs },
          {
            onSuccess: (data) => {
              dispatch({ type: "update_explain", id: entryId, explainData: data });
            },
          }
        );
      }
    },
    [sqlMutation, explainMutation, saveHistory, resultLimit, sqlTimeout]
  );

  const handleClearStream = useCallback(() => {
    dispatch({ type: "clear" });
    setActiveId(null);
  }, []);

  useHotkeys([
    ["mod+l", handleClearStream],
    ["mod+h", () => setHistoryOpen((v) => !v)],
  ]);

  return (
    <div className={`${classes.container} page-enter`}>
      {/* Toolbar */}
      <div className={classes.toolbar}>
        <div className={classes.toolbarTitle}>
          <span className={classes.toolbarMark} />
          <Text size="sm" fw={700}>
            SQL Console
          </Text>
          <Badge size="xs" variant="light" color="primary">
            readonly
          </Badge>
        </div>
        <div className={classes.toolbarActions}>
          <Tooltip label="Query history (⌘H)">
            <button
              type="button"
              className={classes.toolbarButton}
              onClick={() => setHistoryOpen(true)}
            >
              <HistoryIcon size={14} />
              <span>History</span>
              {stream.length > 0 && <span className={classes.toolbarCount}>{stream.length}</span>}
            </button>
          </Tooltip>
          <Tooltip label="Clear session (⌘L)">
            <button type="button" className={classes.shortcutButton} onClick={handleClearStream}>
              <Kbd size="xs">⌘L</Kbd>
            </button>
          </Tooltip>
          <ActiveQueriesDropdown />
        </div>
      </div>

      {/* Workspace: editor over results */}
      <div className={classes.workspace}>
        <Resizable.Group orientation="vertical">
          <Resizable.Pane defaultSize={45} minSize={20}>
            <div className={classes.editorPane}>
              <QueryEditor
                initialQuery={INITIAL_QUERY}
                onQueryChange={handleQueryChange}
                resultLimit={resultLimit}
                onResultLimitChange={setResultLimit}
                sqlTimeout={sqlTimeout}
                onSqlTimeoutChange={setSqlTimeout}
                onExecute={handleExecute}
                onEditorMount={handleEditorMount}
                editorInstance={editorInstance}
                model={modelInstance}
                isPending={sqlMutation.isPending}
              />
            </div>
          </Resizable.Pane>

          <Resizable.Handle />

          <Resizable.Pane defaultSize={55} minSize={20}>
            <div className={classes.resultsHost}>
              <ResultPanel entry={activeEntry} />
            </div>
          </Resizable.Pane>
        </Resizable.Group>
      </div>

      <HistoryDrawer
        open={historyOpen}
        onClose={() => setHistoryOpen(false)}
        entries={stream}
        activeId={activeId}
        onSelect={handleSelectHistory}
      />
    </div>
  );
}
