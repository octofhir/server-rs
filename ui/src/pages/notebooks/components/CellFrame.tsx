import { ActionIcon, Select, TextInput, Tooltip } from "@octofhir/ui-kit";
import { ChevronDown, ChevronRight, Copy, MoveDown, MoveUp, Play, Trash2 } from "lucide-react";
import type { Cell, CellStatus, CellType, Output, Scope } from "../model/notebook";
import classes from "../NotebookEditor.module.css";
import { CellEditor } from "./CellEditor";
import { CellOutput } from "./CellOutput";

const TYPE_LABELS: Record<CellType, string> = {
  markdown: "Markdown",
  fhirpath: "FHIRPath",
  "sql-on-fhir": "SQL-on-FHIR",
  sql: "SQL",
  cql: "CQL",
  graphql: "GraphQL",
  rest: "REST",
  chart: "Chart",
  pipeline: "Pipeline",
  input: "Input",
};

const TYPE_OPTIONS = (["markdown", "fhirpath", "sql", "sql-on-fhir", "chart"] as CellType[]).map(
  (t) => ({ value: t, label: TYPE_LABELS[t] })
);

const STATUS_CLASS: Record<CellStatus, string> = {
  idle: classes.dotIdle,
  running: classes.dotRunning,
  stale: classes.dotStale,
  ok: classes.dotOk,
  error: classes.dotError,
};

interface Props {
  cell: Cell;
  status: CellStatus;
  scope: Scope;
  namedCells: { id: string; label: string }[];
  isFirst: boolean;
  isLast: boolean;
  onChange: (next: Cell) => void;
  onChangeType: (type: CellType) => void;
  onRun: () => void;
  onDelete: () => void;
  onDuplicate: () => void;
  onMove: (dir: -1 | 1) => void;
  onToggleCollapse: () => void;
}

export function CellFrame({
  cell,
  status,
  scope,
  namedCells,
  isFirst,
  isLast,
  onChange,
  onChangeType,
  onRun,
  onDelete,
  onDuplicate,
  onMove,
  onToggleCollapse,
}: Props) {
  const collapsed = cell.collapsed ?? false;
  const outputs: Output[] = cell.outputs ?? [];
  const isMarkdown = cell.type === "markdown";

  return (
    <div className={`${classes.cell} ${status === "error" ? classes.cellError : ""}`}>
      <div className={classes.cellGutter}>
        <button
          type="button"
          className={classes.collapseBtn}
          onClick={onToggleCollapse}
          title={collapsed ? "Expand" : "Collapse"}
        >
          {collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
        </button>
        <span className={`${classes.statusDot} ${STATUS_CLASS[status]}`} />
        <span className={classes.execCount}>{cell.execCount ? `[${cell.execCount}]` : "[ ]"}</span>
      </div>

      <div className={classes.cellMain}>
        <div className={classes.cellToolbar}>
          <Select
            data={TYPE_OPTIONS}
            value={cell.type}
            onChange={(v) => v && onChangeType(v as CellType)}
            size="sm"
          />
          <TextInput
            value={cell.name ?? ""}
            onChange={(v) => onChange({ ...cell, name: (v as string) || undefined } as Cell)}
            placeholder="name (bind output)"
            size="sm"
            className={classes.nameInput}
          />
          <span className={classes.toolbarSpacer} />
          {!isMarkdown && (
            <Tooltip label="Run (Ctrl/⌘+Enter)">
              <ActionIcon variant="subtle" onClick={onRun} aria-label="Run">
                <Play size={15} />
              </ActionIcon>
            </Tooltip>
          )}
          <Tooltip label="Move up">
            <ActionIcon
              variant="subtle"
              disabled={isFirst}
              onClick={() => onMove(-1)}
              aria-label="Move up"
            >
              <MoveUp size={15} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label="Move down">
            <ActionIcon
              variant="subtle"
              disabled={isLast}
              onClick={() => onMove(1)}
              aria-label="Move down"
            >
              <MoveDown size={15} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label="Duplicate">
            <ActionIcon variant="subtle" onClick={onDuplicate} aria-label="Duplicate">
              <Copy size={15} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label="Delete">
            <ActionIcon variant="subtle" onClick={onDelete} aria-label="Delete">
              <Trash2 size={15} />
            </ActionIcon>
          </Tooltip>
        </div>

        {!collapsed && (
          <>
            <div className={isMarkdown ? undefined : classes.cellEditor}>
              <CellEditor cell={cell} onChange={onChange} onRun={onRun} namedCells={namedCells} />
            </div>
            {!isMarkdown && outputs.length > 0 && (
              <div className={classes.cellOutput}>
                {outputs.map((o, i) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: outputs are positional
                  <CellOutput key={i} output={o} scope={scope} />
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
