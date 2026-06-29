import { Select } from "@octofhir/ui-kit";
import { FhirPathEditor } from "@/shared/monaco/FhirPathEditor";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { SqlEditor } from "@/shared/monaco/SqlEditor";
import type { Cell } from "../model/notebook";
import classes from "../NotebookEditor.module.css";
import { MarkdownCellEditor } from "./MarkdownCellEditor";

interface Props {
  cell: Cell;
  onChange: (next: Cell) => void;
  onRun: () => void;
  namedCells: { id: string; label: string }[];
}

export function CellEditor({ cell, onChange, onRun, namedCells }: Props) {
  switch (cell.type) {
    case "markdown":
      return (
        <MarkdownCellEditor
          value={cell.source}
          onChange={(v) => onChange({ ...cell, source: v })}
        />
      );

    case "fhirpath":
      return (
        <div className={classes.editorHost}>
          <FhirPathEditor
            value={cell.source}
            onChange={(v) => onChange({ ...cell, source: v })}
            onSubmit={onRun}
            height="100%"
            placeholder="FHIRPath expression"
          />
        </div>
      );

    case "sql":
      return (
        <div className={classes.editorHostTall}>
          <SqlEditor
            value={cell.source}
            onChange={(v) => onChange({ ...cell, source: v })}
            onExecute={onRun}
            height="100%"
          />
        </div>
      );

    case "sql-on-fhir":
      return (
        <div className={classes.editorHostTall}>
          <JsonEditor
            value={JSON.stringify(cell.source, null, 2)}
            onChange={(v) => {
              try {
                onChange({ ...cell, source: JSON.parse(v) });
              } catch {
                /* keep last valid until parseable */
              }
            }}
            onExecute={onRun}
            height="100%"
          />
        </div>
      );

    case "chart":
      return (
        <div className={classes.chartConfig}>
          <span className={classes.fieldLabel}>Data from cell</span>
          <Select
            data={namedCells.map((c) => ({ value: c.id, label: c.label }))}
            value={cell.source.inputCell}
            onChange={(v) => onChange({ ...cell, source: { ...cell.source, inputCell: v ?? "" } })}
            placeholder="Pick a named table cell"
            size="sm"
          />
          <span className={classes.hintInline}>
            Chart builder wiring lands in P2 — for now renders the referenced table.
          </span>
        </div>
      );

    default:
      return null;
  }
}
