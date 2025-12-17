import { useState, useMemo, useRef, useCallback } from "react";
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
  Box,
  ActionIcon,
  Tooltip,
  Menu,
  useMantineColorScheme,
  useMantineTheme,
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
  IconTemplate,
  IconKeyboard,
} from "@tabler/icons-react";
import { SqlEditor } from "@/shared/monaco/SqlEditor";
import { useSqlMutation } from "@/shared/api/hooks";
import type { SqlValue, FhirOperationOutcome } from "@/shared/api/types";
import { ApiResponseError } from "@/shared/api/serverApi";

/** Query templates for common operations */
const QUERY_TEMPLATES = [
  {
    label: "Select Patients",
    query: "SELECT id, resource->>'name' as name, resource->>'birthDate' as birth_date\nFROM patient\nLIMIT 10;",
  },
  {
    label: "Count Resources by Type",
    query: "SELECT table_name as resource_type, \n  (xpath('/row/cnt/text()', \n    query_to_xml('SELECT COUNT(*) as cnt FROM ' || table_name, false, true, '')))[1]::text::int as count\nFROM information_schema.tables\nWHERE table_schema = 'public'\nORDER BY count DESC;",
  },
  {
    label: "Recent Resources",
    query: "SELECT id, resource_type, created_at, updated_at\nFROM (\n  SELECT id, 'Patient' as resource_type, created_at, updated_at FROM patient\n  UNION ALL\n  SELECT id, 'Observation' as resource_type, created_at, updated_at FROM observation\n) all_resources\nORDER BY updated_at DESC\nLIMIT 20;",
  },
  {
    label: "EXPLAIN Query Plan",
    query: "EXPLAIN ANALYZE\nSELECT * FROM patient WHERE id = 'example-id';",
  },
  {
    label: "Search Parameters",
    query: "SELECT code, type, expression, target\nFROM search_parameters\nWHERE resource_type = 'Patient'\nLIMIT 20;",
  },
];

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
  const sqlMutation = useSqlMutation();
  const theme = useMantineTheme();
  const { colorScheme } = useMantineColorScheme();

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

  const handleLoadTemplate = useCallback((template: string) => {
    queryRef.current = template;
    handleExecute(template);
  }, [handleExecute]);

  const handleExport = useCallback(() => {
    if (sqlMutation.data && sqlMutation.data.rowCount > 0) {
      exportToCSV(sqlMutation.data.columns, sqlMutation.data.rows);
    }
  }, [sqlMutation.data]);

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
        <Badge size="xs" color={value ? "green" : "gray"}>
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
          <Tooltip label="Query Templates">
            <Menu shadow="md" width={280}>
              <Menu.Target>
                <ActionIcon variant="light" size="lg" color={theme.primaryColor}>
                  <IconTemplate size={18} />
                </ActionIcon>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Label>Query Templates</Menu.Label>
                {QUERY_TEMPLATES.map((template) => (
                  <Menu.Item
                    key={template.label}
                    onClick={() => handleLoadTemplate(template.query)}
                  >
                    {template.label}
                  </Menu.Item>
                ))}
              </Menu.Dropdown>
            </Menu>
          </Tooltip>

          <Tooltip label="Query History">
            <Menu shadow="md" width={280}>
              <Menu.Target>
                <ActionIcon variant="light" size="lg" color={theme.primaryColor}>
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
                      onClick={() => handleLoadTemplate(query)}
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
              color={theme.primaryColor}
              onClick={toggleShortcuts}
            >
              <IconKeyboard size={18} />
            </ActionIcon>
          </Tooltip>

          <Button onClick={() => handleExecute()} loading={sqlMutation.isPending} size="md">
            Execute (Ctrl+Enter)
          </Button>
        </Group>
      </Group>

      {/* Keyboard shortcuts help */}
      <Collapse in={shortcutsOpened}>
        <Alert icon={<IconKeyboard size={16} />} title="Keyboard Shortcuts" color="blue">
          <Stack gap="xs">
            <Group gap="xs">
              <Badge variant="light" size="sm">Ctrl+Enter</Badge>
              <Text size="sm">Execute query</Text>
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
        withBorder
        p={0}
        style={{ flex: "0 0 300px", overflow: "hidden" }}
      >
        <Group
          px="sm"
          py="xs"
          justify="space-between"
          style={{
            backgroundColor: colorScheme === "dark"
              ? theme.colors.dark[6]
              : theme.colors.gray[0],
            borderBottom: `1px solid ${colorScheme === "dark"
              ? theme.colors.dark[4]
              : theme.colors.gray[3]}`,
          }}
        >
          <Text size="xs" fw={500} c="dimmed">
            SQL Editor
          </Text>
        </Group>
        <div style={{ height: "260px" }}>
          <SqlEditor
            defaultValue={initialQuery}
            onChange={handleQueryChange}
            onExecute={handleExecute}
            enableLsp
          />
        </div>
      </Paper>

      <Paper
        withBorder
        p="md"
        style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column" }}
      >
        <Group justify="space-between" mb="sm">
          <Text fw={500}>Results</Text>
          <Group gap="xs">
            {sqlMutation.data && (
              <>
                <Text size="sm" c="dimmed">
                  {sqlMutation.data.rowCount} rows in {sqlMutation.data.executionTimeMs}ms
                </Text>
                {sqlMutation.data.rowCount > 0 && (
                  <Tooltip label="Export to CSV">
                    <ActionIcon
                      variant="light"
                      size="sm"
                      color={theme.primaryColor}
                      onClick={handleExport}
                    >
                      <IconDownload size={16} />
                    </ActionIcon>
                  </Tooltip>
                )}
              </>
            )}
          </Group>
        </Group>

        <div style={{ flex: 1, overflow: "auto" }}>
          {errorMessage && (
            <Stack gap="sm">
              <Alert icon={<IconAlertCircle size={16} />} color="red" title="Query Error">
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
            <Alert icon={<IconInfoCircle size={16} />} color="blue">
              Query executed successfully. No rows returned.
            </Alert>
          )}

          {/* EXPLAIN output - render as formatted code block */}
          {sqlMutation.data && sqlMutation.data.rowCount > 0 && isExplain && (
            <Box>
              <Badge color="violet" size="sm" mb="sm">
                Query Plan
              </Badge>
              <ScrollArea>
                <Code
                  block
                  style={{
                    whiteSpace: "pre",
                    fontFamily: "var(--mantine-font-family-monospace)",
                    fontSize: 13,
                  }}
                >
                  {explainText}
                </Code>
              </ScrollArea>
            </Box>
          )}

          {/* Regular table results */}
          {sqlMutation.data && sqlMutation.data.rowCount > 0 && !isExplain && (
            <ScrollArea>
              <Table striped highlightOnHover withTableBorder>
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
      </Paper>
    </Stack>
  );
}
