import {
  Badge,
  Collapse,
  Group,
  Paper,
  Stack,
  Tabs,
  Text,
  UnstyledButton,
} from "@mantine/core";
import { IconChevronDown, IconChevronUp } from "@tabler/icons-react";
import type * as monaco from "monaco-editor";
import { useCallback, useState } from "react";
import {
  type DiagnosticsByLevel,
  getDiagnosticsCount,
  useLspDiagnostics,
} from "@/shared/monaco/lib/useLspDiagnostics";
import { DiagnosticItem } from "./DiagnosticItem";

interface DiagnosticsPanelProps {
  /** Monaco editor model to track diagnostics for */
  model: monaco.editor.ITextModel | null;
  /** Monaco editor instance for navigation */
  editor: monaco.editor.IStandaloneCodeEditor | null;
  /** Initial collapsed state */
  defaultCollapsed?: boolean;
  /** Height of the panel when expanded */
  height?: number | string;
}

/**
 * VS Code-style diagnostics panel for displaying LSP diagnostics
 * Shows errors, warnings, info, and hints with click-to-navigate functionality
 */
type DiagnosticTab = "all" | "errors" | "warnings" | "info" | "hints";

export function DiagnosticsPanel({
  model,
  editor,
  defaultCollapsed = false,
  height = 200,
}: DiagnosticsPanelProps) {
  const [collapsed, setCollapsed] = useState(defaultCollapsed);
  const [activeTab, setActiveTab] = useState<DiagnosticTab>("all");

  const diagnostics = useLspDiagnostics(model);
  const totalCount = getDiagnosticsCount(diagnostics);

  const toggleCollapsed = useCallback(() => {
    setCollapsed((prev) => !prev);
  }, []);

  const handleDiagnosticClick = useCallback(
    (lineNumber: number, column: number) => {
      if (!editor) return;

      // Navigate to the diagnostic location
      editor.setPosition({ lineNumber, column });
      editor.revealLineInCenter(lineNumber);
      editor.focus();
    },
    [editor]
  );

  const renderDiagnosticList = (items: DiagnosticsByLevel[keyof DiagnosticsByLevel]) => {
    if (items.length === 0) {
      return (
        <Text c="dimmed" size="sm" ta="center" py="xl">
          No problems detected
        </Text>
      );
    }

    return (
      <Stack gap={0}>
        {items.map((diagnostic, index) => (
          <DiagnosticItem
            key={`${index}-${diagnostic.startLineNumber}-${diagnostic.message}`}
            diagnostic={diagnostic}
            onClick={() =>
              handleDiagnosticClick(diagnostic.startLineNumber, diagnostic.startColumn)
            }
          />
        ))}
      </Stack>
    );
  };

  // Get diagnostics to display based on active tab
  const getFilteredDiagnostics = () => {
    switch (activeTab) {
      case "errors":
        return diagnostics.errors;
      case "warnings":
        return diagnostics.warnings;
      case "info":
        return diagnostics.info;
      case "hints":
        return diagnostics.hints;
      default:
        return [
          ...diagnostics.errors,
          ...diagnostics.warnings,
          ...diagnostics.info,
          ...diagnostics.hints,
        ];
    }
  };

  return (
    <Paper
      style={{
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
        backgroundColor: "var(--app-surface-1)",
      }}
    >
      {/* Header */}
      <UnstyledButton
        onClick={toggleCollapsed}
        style={{
          padding: "8px 12px",
          backgroundColor: "var(--app-surface-2)",
        }}
      >
        <Group justify="space-between">
          <Group gap="xs">
            {collapsed ? <IconChevronUp size={16} /> : <IconChevronDown size={16} />}
            <Text size="sm" fw={500}>
              Problems
            </Text>
            {totalCount > 0 && (
              <Badge size="sm" variant="light">
                {totalCount}
              </Badge>
            )}
          </Group>
          <Group gap="xs">
            {diagnostics.errors.length > 0 && (
              <Badge size="sm" color="fire" variant="light">
                {diagnostics.errors.length} errors
              </Badge>
            )}
            {diagnostics.warnings.length > 0 && (
              <Badge size="sm" color="warm" variant="light">
                {diagnostics.warnings.length} warnings
              </Badge>
            )}
            {diagnostics.info.length > 0 && (
              <Badge size="sm" color="primary" variant="light">
                {diagnostics.info.length} info
              </Badge>
            )}
            {diagnostics.hints.length > 0 && (
              <Badge size="sm" color="deep" variant="light">
                {diagnostics.hints.length} hints
              </Badge>
            )}
          </Group>
        </Group>
      </UnstyledButton>

      {/* Collapsible content with tabs */}
      <Collapse in={!collapsed}>
        <Tabs
          value={activeTab}
          onChange={(value) => setActiveTab(value as DiagnosticTab)}
          variant="pills"
          styles={{
            root: {
              height: typeof height === "number" ? `${height}px` : height,
              display: "flex",
              flexDirection: "column",
            },
            list: {
              padding: "8px 12px",
            },
            panel: {
              flex: 1,
              overflowY: "auto",
              padding: "12px",
            },
          }}
        >
          <Tabs.List>
            <Tabs.Tab value="all">
              All
              {totalCount > 0 && (
                <Badge size="xs" ml={6} variant="light">
                  {totalCount}
                </Badge>
              )}
            </Tabs.Tab>
            <Tabs.Tab value="errors" disabled={diagnostics.errors.length === 0}>
              Errors
              {diagnostics.errors.length > 0 && (
                <Badge size="xs" ml={6} color="fire" variant="light">
                  {diagnostics.errors.length}
                </Badge>
              )}
            </Tabs.Tab>
            <Tabs.Tab value="warnings" disabled={diagnostics.warnings.length === 0}>
              Warnings
              {diagnostics.warnings.length > 0 && (
                <Badge size="xs" ml={6} color="warm" variant="light">
                  {diagnostics.warnings.length}
                </Badge>
              )}
            </Tabs.Tab>
            <Tabs.Tab value="info" disabled={diagnostics.info.length === 0}>
              Info
              {diagnostics.info.length > 0 && (
                <Badge size="xs" ml={6} color="primary" variant="light">
                  {diagnostics.info.length}
                </Badge>
              )}
            </Tabs.Tab>
            <Tabs.Tab value="hints" disabled={diagnostics.hints.length === 0}>
              Hints
              {diagnostics.hints.length > 0 && (
                <Badge size="xs" ml={6} color="deep" variant="light">
                  {diagnostics.hints.length}
                </Badge>
              )}
            </Tabs.Tab>
          </Tabs.List>

          <Tabs.Panel value="all">{renderDiagnosticList(getFilteredDiagnostics())}</Tabs.Panel>

          <Tabs.Panel value="errors">{renderDiagnosticList(diagnostics.errors)}</Tabs.Panel>

          <Tabs.Panel value="warnings">{renderDiagnosticList(diagnostics.warnings)}</Tabs.Panel>

          <Tabs.Panel value="info">{renderDiagnosticList(diagnostics.info)}</Tabs.Panel>

          <Tabs.Panel value="hints">{renderDiagnosticList(diagnostics.hints)}</Tabs.Panel>
        </Tabs>
      </Collapse>
    </Paper>
  );
}
