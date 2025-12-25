import { useState, useMemo, useRef, useCallback, useEffect } from "react";
import {
  Stack,
  Title,
  Text,
  Group,
  Button,
  Paper,
  Table,
  Alert,
  Code,
  ScrollArea,
  Collapse,
  UnstyledButton,
  Popover,
  Badge,
  ActionIcon,
  Tooltip,
  Menu,
} from "@mantine/core";
import { useDisclosure, useLocalStorage } from "@mantine/hooks";
import {
  IconAlertCircle,
  IconInfoCircle,
  IconChevronDown,
  IconChevronRight,
  IconBraces,
  IconDownload,
  IconClock,
  IconKeyboard,
  IconCode,
  IconSettings,
} from "@tabler/icons-react";
import type * as monaco from "monaco-editor";
import { SqlEditor } from "@/shared/monaco/SqlEditor";
import { useSqlMutation, useFormatterSettings } from "@/shared/api/hooks";
import type { SqlValue, FhirOperationOutcome } from "@/shared/api/types";
import { ApiResponseError } from "@/shared/api/serverApi";
import { DiagnosticsPanel } from "@/widgets/diagnostics-panel";
import { ExplainVisualization } from "@/widgets/explain-visualization";
import { setLspFormatterConfig } from "@/shared/monaco/lspClient";
import { FormatterSettings } from "@/shared/settings/FormatterSettings";

/** Export results to CSV */
function exportToCSV(columns: string[], rows: SqlValue[][]): void {
  const csvHeader = columns.join(",");
  const csvRows = rows.map(row =>
    row.map(cell => {
      if (cell === null) return "";
      if (typeof cell === "object") return JSON.stringify(cell);
      const str = String(cell);
      // Escape quotes and wrap in quotes if contains comma, quote, or newline
      if (str.includes(",") || str.includes('"') || str.includes("\n")) {
        return `"${str.replace(/"/g, '""')}"`;
      }
      return str;
    }).join(",")
  );

  const csv = [csvHeader, ...csvRows].join("\n");
  const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
  const link = document.createElement("a");
  const url = URL.createObjectURL(blob);

  link.setAttribute("href", url);
  link.setAttribute("download", `query-results-${Date.now()}.csv`);
  link.style.visibility = "hidden";
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
}

/** Component to display JSON values in a cell with popover for full view */
function JsonCell({ value }: { value: Record<string, unknown> }) {
  const [opened, { open, close }] = useDisclosure(false);
  const jsonString = JSON.stringify(value, null, 2);
  const preview = JSON.stringify(value);
  const isLarge = preview.length > 50;

  return (
    <Popover opened={opened} position="bottom-start" withArrow shadow="md" width={400}>
      <Popover.Target>
        <UnstyledButton
          onMouseEnter={open}
          onMouseLeave={close}
          style={{ display: "flex", alignItems: "center", gap: 4 }}
        >
          <IconBraces size={14} style={{ opacity: 0.5 }} />
          <Text
            size="sm"
            truncate
            style={{ maxWidth: 200, fontFamily: "var(--mantine-font-family-monospace)" }}
          >
            {isLarge ? `${preview.slice(0, 47)}...` : preview}
          </Text>
        </UnstyledButton>
      </Popover.Target>
      <Popover.Dropdown>
        <ScrollArea.Autosize mah={300}>
          <Code block style={{ fontSize: 12 }}>
            {jsonString}
          </Code>
        </ScrollArea.Autosize>
      </Popover.Dropdown>
    </Popover>
  );
}

/** Check if the result is from an EXPLAIN query */
function isExplainResult(columns: string[]): boolean {
  return columns.length === 1 && columns[0].toUpperCase() === "QUERY PLAN";
}

export function DbConsolePage() {
  const initialQuery = "SELECT * FROM patient LIMIT 10;";
  const queryRef = useRef(initialQuery);
  const [showErrorDetails, setShowErrorDetails] = useState(false);
  const [shortcutsOpened, { toggle: toggleShortcuts }] = useDisclosure(false);
  const [resultsCollapsed, { toggle: toggleResults }] = useDisclosure(false);
  const [formatterOpened, { toggle: toggleFormatter, close: closeFormatter }] = useDisclosure(false);
  const [editorHeight, setEditorHeight] = useState(300);
  const sqlMutation = useSqlMutation();
  // State for Monaco editor and model (for DiagnosticsPanel)
  const [editorInstance, setEditorInstance] = useState<monaco.editor.IStandaloneCodeEditor | null>(null);
  const [modelInstance, setModelInstance] = useState<monaco.editor.ITextModel | null>(null);

  // Formatter configuration
  const { config: formatterConfig, saveConfig: saveFormatterConfig } = useFormatterSettings();

  // Sync formatter config to LSP client when it changes
  useEffect(() => {
    setLspFormatterConfig(formatterConfig);
  }, [formatterConfig]);

  // Query history stored in localStorage
  const [queryHistory, setQueryHistory] = useLocalStorage<string[]>({
    key: "db-console-history",
    defaultValue: [],
  });

  const handleQueryChange = useCallback((value: string) => {
    queryRef.current = value;
  }, []);

  const handleExecute = useCallback(
    (value?: string) => {
      const queryToRun = value ?? queryRef.current;
      queryRef.current = queryToRun;
      sqlMutation.mutate({ query: queryToRun });

      // Add to history (keep last 20 unique queries)
      setQueryHistory((prev) => {
        const updated = [queryToRun, ...prev.filter((q) => q !== queryToRun)];
        return updated.slice(0, 20);
      });
    },
    [sqlMutation, setQueryHistory],
  );

  const handleLoadQueryFromHistory = useCallback((query: string) => {
    // Set the query in the editor without executing
    if (editorInstance) {
      editorInstance.setValue(query);
      queryRef.current = query;
      editorInstance.focus();
    }
  }, [editorInstance]);

  const handleExport = useCallback(() => {
    if (sqlMutation.data && sqlMutation.data.rowCount > 0) {
      exportToCSV(sqlMutation.data.columns, sqlMutation.data.rows);
    }
  }, [sqlMutation.data]);

  const handleEditorMount = useCallback((editor: monaco.editor.IStandaloneCodeEditor, model: monaco.editor.ITextModel) => {
    setEditorInstance(editor);
    setModelInstance(model);
  }, []);

  const handleFormat = useCallback(() => {
    if (editorInstance) {
      editorInstance.getAction('editor.action.formatDocument')?.run();
    }
  }, [editorInstance]);

  // Handle editor resize via drag
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const startY = e.clientY;
    const startHeight = editorHeight;

    const handleMouseMove = (moveEvent: MouseEvent) => {
      const delta = moveEvent.clientY - startY;
      const newHeight = Math.max(200, Math.min(800, startHeight + delta));
      setEditorHeight(newHeight);
    };

    const handleMouseUp = () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
  }, [editorHeight]);

  /** Check if this is an EXPLAIN result */
  const isExplain = useMemo(() => {
    if (!sqlMutation.data) return false;
    return isExplainResult(sqlMutation.data.columns);
  }, [sqlMutation.data]);

  /** Format EXPLAIN output as readable text */
  const explainText = useMemo(() => {
    if (!isExplain || !sqlMutation.data) return "";
    return sqlMutation.data.rows.map((row) => String(row[0])).join("\n");
  }, [isExplain, sqlMutation.data]);

  /** Render a cell value appropriately based on type */
  const renderCellValue = (value: SqlValue): React.ReactNode => {
    if (value === null)
      return (
        <Text span c="dimmed" fs="italic">
          NULL
        </Text>
      );
    if (typeof value === "object" && value !== null) {
      return <JsonCell value={value as Record<string, unknown>} />;
    }
    if (typeof value === "boolean") {
      return (
        <Badge size="xs" color={value ? "primary" : "deep"}>
          {value.toString()}
        </Badge>
      );
    }
    return String(value);
  };

  // Extract error details
  const errorMessage = sqlMutation.error
    ? sqlMutation.error instanceof ApiResponseError
      ? sqlMutation.error.responseData?.resourceType === "OperationOutcome"
        ? (sqlMutation.error.responseData as FhirOperationOutcome).issue?.[0]?.diagnostics ||
          sqlMutation.error.message
        : sqlMutation.error.message
      : sqlMutation.error.message
    : null;

  const operationOutcome =
    sqlMutation.error instanceof ApiResponseError &&
    sqlMutation.error.responseData?.resourceType === "OperationOutcome"
      ? (sqlMutation.error.responseData as FhirOperationOutcome)
      : null;

  return (
    <Stack gap="md" h="100%">
      <Group justify="space-between" align="flex-start">
        <div>
          <Title order={2}>DB Console</Title>
          <Text c="dimmed" size="sm">
            Execute SQL queries against the database
          </Text>
        </div>
        <Group gap="xs">
          <Tooltip label="Query History">
            <Menu shadow="md" width={280}>
              <Menu.Target>
                <ActionIcon variant="light" size="lg" color="warm">
                  <IconClock size={18} />
                </ActionIcon>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Label>Recent Queries</Menu.Label>
                {queryHistory.length === 0 ? (
                  <Menu.Item disabled>No history yet</Menu.Item>
                ) : (
                  queryHistory.slice(0, 10).map((query) => (
                    <Menu.Item
                      key={query}
                      onClick={() => handleLoadQueryFromHistory(query)}
                      style={{
                        maxWidth: "260px",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      <Code
                        style={{
                          fontSize: "11px",
                          maxWidth: "100%",
                          display: "block",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        }}
                      >
                        {query}
                      </Code>
                    </Menu.Item>
                  ))
                )}
              </Menu.Dropdown>
            </Menu>
          </Tooltip>

          <Tooltip label="Keyboard Shortcuts">
            <ActionIcon
              variant="light"
              size="lg"
              color="warm"
              onClick={toggleShortcuts}
            >
              <IconKeyboard size={18} />
            </ActionIcon>
          </Tooltip>

          <Tooltip label="Format SQL (Shift+Alt+F)">
            <ActionIcon
              variant="light"
              size="lg"
              color="warm"
              onClick={handleFormat}
              disabled={!editorInstance}
            >
              <IconCode size={18} />
            </ActionIcon>
          </Tooltip>

          <Popover
            opened={formatterOpened}
            onClose={closeFormatter}
            position="bottom-end"
            width={320}
            shadow="md"
          >
            <Popover.Target>
              <Tooltip label="Formatter Settings">
                <ActionIcon
                  variant="light"
                  size="lg"
                  color="warm"
                  onClick={toggleFormatter}
                >
                  <IconSettings size={18} />
                </ActionIcon>
              </Tooltip>
            </Popover.Target>
            <Popover.Dropdown>
              <Text size="sm" fw={500} mb="sm">
                SQL Formatter Settings
              </Text>
              <FormatterSettings
                value={formatterConfig}
                onChange={saveFormatterConfig}
                compact
              />
            </Popover.Dropdown>
          </Popover>

          <Button onClick={() => handleExecute()} loading={sqlMutation.isPending}>
            Execute (Ctrl+Enter)
          </Button>
        </Group>
      </Group>

      {/* Keyboard shortcuts help */}
      <Collapse in={shortcutsOpened}>
        <Alert icon={<IconKeyboard size={16} />} title="Keyboard Shortcuts" color="primary">
          <Stack gap="xs">
            <Group gap="xs">
              <Badge variant="light" size="sm">Ctrl+Enter</Badge>
              <Text size="sm">Execute query</Text>
            </Group>
            <Group gap="xs">
              <Badge variant="light" size="sm">Shift+Alt+F</Badge>
              <Text size="sm">Format SQL</Text>
            </Group>
            <Group gap="xs">
              <Badge variant="light" size="sm">Ctrl+Space</Badge>
              <Text size="sm">Trigger autocomplete</Text>
            </Group>
            <Group gap="xs">
              <Badge variant="light" size="sm">Hover</Badge>
              <Text size="sm">Show table/column information</Text>
            </Group>
          </Stack>
        </Alert>
      </Collapse>

      <Paper
        p={0}
        style={{
          flex: `0 0 ${editorHeight}px`,
          overflow: "hidden",
          display: "flex",
          flexDirection: "column",
          position: "relative",
          backgroundColor: "var(--app-surface-1)",
        }}
      >
        <Group
          px="sm"
          py="xs"
          justify="space-between"
          style={{ backgroundColor: "var(--app-surface-2)" }}
        >
          <Text size="xs" fw={500} c="dimmed">
            SQL Editor
          </Text>
          <Text size="xs" c="dimmed">
            Drag bottom edge to resize
          </Text>
        </Group>
        <div style={{ height: `${editorHeight - 40}px` }}>
          <SqlEditor
            defaultValue={initialQuery}
            onChange={handleQueryChange}
            onExecute={handleExecute}
            onEditorMount={handleEditorMount}
            enableLsp
          />
        </div>
        {/* Resize handle */}
        <UnstyledButton
          aria-label="Resize SQL editor (drag or use arrow keys)"
          onMouseDown={handleMouseDown}
          onKeyDown={(e) => {
            if (e.key === "ArrowUp") {
              setEditorHeight((prev) => Math.max(200, prev - 20));
            } else if (e.key === "ArrowDown") {
              setEditorHeight((prev) => Math.min(800, prev + 20));
            }
          }}
          style={{
            position: "absolute",
            bottom: 0,
            left: 0,
            right: 0,
            height: "4px",
            cursor: "ns-resize",
            backgroundColor: "var(--app-border-subtle)",
            transition: "background-color 0.2s",
            border: "none",
            padding: 0,
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.backgroundColor = "var(--app-accent-warm)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.backgroundColor = "var(--app-border-subtle)";
          }}
        />
      </Paper>

      {/* Diagnostics Panel */}
      <DiagnosticsPanel
        model={modelInstance}
        editor={editorInstance}
        defaultCollapsed={true}
        height={180}
      />

      <Paper
        p={0}
        style={{
          flex: 1,
          overflow: "hidden",
          display: "flex",
          flexDirection: "column",
          backgroundColor: "var(--app-surface-1)",
        }}
      >
        {/* Results Header (Collapsible) */}
        <UnstyledButton
          onClick={toggleResults}
          style={{
            padding: "12px 16px",
            backgroundColor: "var(--app-surface-2)",
          }}
        >
          <Group justify="space-between">
            <Group gap="xs">
              {resultsCollapsed ? (
                <IconChevronRight size={16} />
              ) : (
                <IconChevronDown size={16} />
              )}
              <Text fw={500}>Results</Text>
              {/* Show row count badge only for regular queries, not EXPLAIN */}
              {sqlMutation.data && !isExplain && (
                <Badge size="sm" variant="light">
                  {sqlMutation.data.rowCount} rows
                </Badge>
              )}
              {/* Show Query Plan badge for EXPLAIN queries */}
              {sqlMutation.data && isExplain && (
                <Badge size="sm" variant="light" color="deep">
                  Query Plan
                </Badge>
              )}
            </Group>
            <Group gap="xs">
              {sqlMutation.data && (
                <>
                  <Text size="sm" c="dimmed">
                    {sqlMutation.data.executionTimeMs}ms
                  </Text>
                  {/* Hide export button for EXPLAIN queries */}
                  {sqlMutation.data.rowCount > 0 && !isExplain && (
                    <Tooltip label="Export to CSV">
                      <ActionIcon
                        variant="light"
                        size="sm"
                        color="warm"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleExport();
                        }}
                      >
                        <IconDownload size={16} />
                      </ActionIcon>
                    </Tooltip>
                  )}
                </>
              )}
            </Group>
          </Group>
        </UnstyledButton>

        <Collapse in={!resultsCollapsed}>
          <div style={{ maxHeight: "calc(100vh - 600px)", overflow: "auto", padding: "16px" }}>
          {errorMessage && (
            <Stack gap="sm">
              <Alert icon={<IconAlertCircle size={16} />} color="fire" title="Query Error">
                {errorMessage}
              </Alert>
              {operationOutcome && (
                <>
                  <UnstyledButton onClick={() => setShowErrorDetails(!showErrorDetails)}>
                    <Group gap="xs">
                      {showErrorDetails ? (
                        <IconChevronDown size={14} />
                      ) : (
                        <IconChevronRight size={14} />
                      )}
                      <Text size="sm" c="dimmed">
                        Show full OperationOutcome
                      </Text>
                    </Group>
                  </UnstyledButton>
                  <Collapse in={showErrorDetails}>
                    <Code block>{JSON.stringify(operationOutcome, null, 2)}</Code>
                  </Collapse>
                </>
              )}
            </Stack>
          )}

          {!sqlMutation.data && !sqlMutation.error && !sqlMutation.isPending && (
            <Text c="dimmed" ta="center" py="xl">
              Run a query to see results
            </Text>
          )}

          {sqlMutation.data?.rowCount === 0 && (
            <Alert icon={<IconInfoCircle size={16} />} color="primary">
              Query executed successfully. No rows returned.
            </Alert>
          )}

          {/* EXPLAIN output - render as interactive tree visualization */}
          {sqlMutation.data && sqlMutation.data.rowCount > 0 && isExplain && (
            <ScrollArea>
              <ExplainVisualization explainText={explainText} />
            </ScrollArea>
          )}

          {/* Regular table results */}
          {sqlMutation.data && sqlMutation.data.rowCount > 0 && !isExplain && (
            <ScrollArea>
              <Table striped highlightOnHover>
                <Table.Thead>
                  <Table.Tr>
                    {sqlMutation.data.columns.map((col) => (
                      <Table.Th key={col}>{col}</Table.Th>
                    ))}
                  </Table.Tr>
                </Table.Thead>
                <Table.Tbody>
                  {sqlMutation.data.rows.map((row, rowIdx) => (
                    <Table.Tr key={`row-${rowIdx}-${JSON.stringify(row[0])}`}>
                      {row.map((cell, cellIdx) => (
                        <Table.Td key={`cell-${rowIdx}-${cellIdx}-${typeof cell === 'object' ? JSON.stringify(cell) : cell}`}>
                          {renderCellValue(cell)}
                        </Table.Td>
                      ))}
                    </Table.Tr>
                  ))}
                </Table.Tbody>
              </Table>
            </ScrollArea>
          )}
          </div>
        </Collapse>
      </Paper>
    </Stack>
  );
}
