import {
  ActionIcon,
  Button,
  Popover,
  ScrollArea,
  Select,
  Text,
  Tooltip,
  useDisclosure,
} from "@octofhir/ui-kit";
import {
  CircleAlert,
  Code,
  Settings as Gear,
  Info,
  Play,
  TriangleAlert,
  X as Xmark,
} from "lucide-react";
import type * as monaco from "monaco-editor";
import { useCallback, useEffect } from "react";
import { useFormatterSettings } from "@/shared/api/hooks";
import { useLspDiagnostics } from "@/shared/monaco/lib/useLspDiagnostics";
import { setLspFormatterConfig } from "@/shared/monaco/lspClient";
import { SqlEditor } from "@/shared/monaco/SqlEditor";
import { FormatterSettings } from "@/shared/settings/FormatterSettings";
import classes from "../DbConsolePage.module.css";
import { DiagnosticsList } from "./DiagnosticsList";

interface QueryEditorProps {
  initialQuery: string;
  onQueryChange: (value: string) => void;
  resultLimit: string;
  onResultLimitChange: (value: string) => void;
  sqlTimeout: string;
  onSqlTimeoutChange: (value: string) => void;
  onExecute: (value?: string) => void;
  onEditorMount: (
    editor: monaco.editor.IStandaloneCodeEditor,
    model: monaco.editor.ITextModel
  ) => void;
  editorInstance: monaco.editor.IStandaloneCodeEditor | null;
  model: monaco.editor.ITextModel | null;
  isPending: boolean;
}

const RESULT_LIMIT_OPTIONS = [
  { value: "50", label: "Limit 50" },
  { value: "100", label: "Limit 100" },
  { value: "200", label: "Limit 200" },
  { value: "500", label: "Limit 500" },
  { value: "1000", label: "Limit 1000" },
  { value: "none", label: "No limit" },
];
const TIMEOUT_OPTIONS = [
  { value: "10000", label: "10s timeout" },
  { value: "30000", label: "30s timeout" },
  { value: "60000", label: "60s timeout" },
  { value: "120000", label: "120s timeout" },
];
const DEFAULT_LIMIT = "200";
const DEFAULT_TIMEOUT = "120000";

export function QueryEditor({
  initialQuery,
  onQueryChange,
  resultLimit,
  onResultLimitChange,
  sqlTimeout,
  onSqlTimeoutChange,
  onExecute,
  onEditorMount,
  editorInstance,
  model,
  isPending,
}: QueryEditorProps) {
  const [formatterOpened, { open: openFormatter, close: closeFormatter }] = useDisclosure(false);
  const [problemsOpen, { toggle: toggleProblems, close: closeProblems }] = useDisclosure(false);
  const { config: formatterConfig, saveConfig: saveFormatterConfig } = useFormatterSettings();

  const diagnostics = useLspDiagnostics(model);
  const errorCount = diagnostics.errors.length;
  const warnCount = diagnostics.warnings.length;
  const infoCount = diagnostics.info.length + diagnostics.hints.length;
  const totalCount = errorCount + warnCount + infoCount;
  const allDiagnostics = [
    ...diagnostics.errors,
    ...diagnostics.warnings,
    ...diagnostics.info,
    ...diagnostics.hints,
  ];

  useEffect(() => {
    setLspFormatterConfig(formatterConfig);
  }, [formatterConfig]);

  // Auto-close the strip once the editor is clean.
  useEffect(() => {
    if (totalCount === 0) closeProblems();
  }, [totalCount, closeProblems]);

  const handleFormat = useCallback(() => {
    editorInstance?.getAction("editor.action.formatDocument")?.run();
  }, [editorInstance]);

  const handleNavigate = useCallback(
    (lineNumber: number, column: number) => {
      if (!editorInstance) return;
      editorInstance.setPosition({ lineNumber, column });
      editorInstance.revealLineInCenter(lineNumber);
      editorInstance.focus();
    },
    [editorInstance]
  );

  return (
    <div className={classes.editorPaneRoot}>
      <div className={classes.editorToolbar}>
        <div className={classes.editorToolbarLeft}>
          <span className={classes.promptGlyphInline}>{`>>>`}</span>
          <Text size="xs" fw={600} c="dimmed">
            Query
          </Text>
          <button
            type="button"
            className={[
              classes.problemsCounter,
              totalCount === 0 ? classes.problemsCounterClean : undefined,
              problemsOpen ? classes.problemsCounterActive : undefined,
            ]
              .filter(Boolean)
              .join(" ")}
            onClick={toggleProblems}
            disabled={totalCount === 0}
            title={totalCount === 0 ? "No problems" : `${totalCount} problems`}
          >
            {totalCount === 0 ? (
              <span className={classes.problemsClean}>No problems</span>
            ) : (
              <>
                {errorCount > 0 && (
                  <span className={classes.problemsErr}>
                    <CircleAlert size={13} />
                    {errorCount}
                  </span>
                )}
                {warnCount > 0 && (
                  <span className={classes.problemsWarn}>
                    <TriangleAlert size={13} />
                    {warnCount}
                  </span>
                )}
                {infoCount > 0 && (
                  <span className={classes.problemsInfo}>
                    <Info size={13} />
                    {infoCount}
                  </span>
                )}
              </>
            )}
          </button>
        </div>
        <div className={classes.editorToolbarRight}>
          <Tooltip label="Format SQL (Shift+Alt+F)">
            <ActionIcon
              variant="subtle"
              size="sm"
              onClick={handleFormat}
              disabled={!editorInstance}
            >
              <Code size={15} />
            </ActionIcon>
          </Tooltip>

          <Popover
            open={formatterOpened}
            onOpenChange={(open) => (open ? openFormatter() : closeFormatter())}
            placement="bottom-end"
            trigger="click"
            content={
              <div className={classes.formatterPopover}>
                <Text size="sm" fw={500} mb="sm">
                  SQL Formatter Settings
                </Text>
                <FormatterSettings
                  value={formatterConfig}
                  onChange={(config) => {
                    setLspFormatterConfig(config);
                    saveFormatterConfig(config);
                  }}
                  compact
                />
                <div className={classes.popoverActions}>
                  <Button
                    size="xs"
                    variant="light"
                    onClick={() => {
                      closeFormatter();
                      handleFormat();
                    }}
                  >
                    Apply & Format
                  </Button>
                </div>
              </div>
            }
          >
            <Tooltip label="Formatter settings">
              <ActionIcon variant="subtle" size="sm">
                <Gear size={15} />
              </ActionIcon>
            </Tooltip>
          </Popover>

          <Tooltip label="Auto-limit for SELECT/WITH queries without LIMIT">
            <Select
              size="xs"
              w={112}
              value={resultLimit}
              onChange={(value) => onResultLimitChange(value ?? DEFAULT_LIMIT)}
              data={RESULT_LIMIT_OPTIONS}
              aria-label="SQL result limit"
            />
          </Tooltip>

          <Tooltip label="Client-side timeout for SQL execution">
            <Select
              size="xs"
              w={126}
              value={sqlTimeout}
              onChange={(value) => onSqlTimeoutChange(value ?? DEFAULT_TIMEOUT)}
              data={TIMEOUT_OPTIONS}
              aria-label="SQL timeout"
            />
          </Tooltip>

          <Button
            size="xs"
            onClick={() => onExecute()}
            loading={isPending}
            leftSection={<Play size={13} />}
          >
            Run
            <Text as="span" size="xs" c="dimmed" ml={6}>
              ⌘↩
            </Text>
          </Button>
        </div>
      </div>

      <div className={classes.editorBody}>
        <SqlEditor
          defaultValue={initialQuery}
          onChange={onQueryChange}
          onExecute={onExecute}
          onEditorMount={onEditorMount}
          enableLsp
        />
      </div>

      {problemsOpen && totalCount > 0 && (
        <div className={classes.problemsStrip}>
          <div className={classes.problemsStripHead}>
            <Text size="xs" fw={700}>
              Problems
            </Text>
            <span className={classes.problemsStripCount}>{totalCount}</span>
            <Tooltip label="Hide problems">
              <ActionIcon
                variant="subtle"
                size="xs"
                onClick={closeProblems}
                className={classes.problemsStripClose}
              >
                <Xmark size={13} />
              </ActionIcon>
            </Tooltip>
          </div>
          <ScrollArea className={classes.problemsStripList}>
            <DiagnosticsList diagnostics={allDiagnostics} onNavigate={handleNavigate} />
          </ScrollArea>
        </div>
      )}
    </div>
  );
}
