// Chart cell — binds a named table cell's output to the ui-kit ChartBuilder.
// Live: no run button; the builder's preview IS the output. The spec persists in
// the cell; data flows from the reactive scope (the referenced cell's table).

import { ChartBuilder, Select, suggestChartSpec, type TabularData } from "@octofhir/ui-kit";
import { useMemo } from "react";
import type { ChartCell, Scope } from "../model/notebook";
import classes from "../NotebookEditor.module.css";

interface Props {
  cell: ChartCell;
  onChange: (next: ChartCell) => void;
  namedCells: { id: string; name: string; label: string }[];
  scope: Scope;
}

const EMPTY: TabularData = { columns: [], rows: [] };

function asTable(value: unknown): TabularData {
  if (value && typeof value === "object" && "columns" in value && "rows" in value) {
    const v = value as { columns: unknown; rows: unknown };
    if (Array.isArray(v.columns) && Array.isArray(v.rows)) {
      return { columns: v.columns as string[], rows: v.rows as unknown[][] };
    }
  }
  return EMPTY;
}

export function ChartCellEditor({ cell, onChange, namedCells, scope }: Props) {
  const inputName = cell.source.inputCell;
  const data = useMemo(() => asTable(scope[inputName]), [scope, inputName]);
  const spec = cell.source.spec;

  return (
    <div className={classes.chartCell}>
      <div className={classes.chartInputRow}>
        <span className={classes.ctxLabel}>Data</span>
        <Select
          data={namedCells.map((c) => ({ value: c.name, label: c.label }))}
          value={inputName || null}
          onChange={(v) => {
            const next = v ?? "";
            // Seed a sensible spec from the newly-picked table so the chart isn't blank.
            const seeded =
              next && (!spec.series?.length || !spec.x)
                ? suggestChartSpec(asTable(scope[next]))
                : spec;
            onChange({ ...cell, source: { inputCell: next, spec: seeded } });
          }}
          placeholder={
            namedCells.length
              ? "Pick a named table cell"
              : "Run a named SQL / SQL-on-FHIR cell first"
          }
          size="sm"
          searchable
          className={classes.chartInputSel}
        />
      </div>

      {inputName && data.rows.length > 0 ? (
        <ChartBuilder
          data={data}
          spec={spec}
          onSpecChange={(s) => onChange({ ...cell, source: { ...cell.source, spec: s } })}
          height={340}
        />
      ) : (
        <div className={classes.chartEmpty}>
          {inputName
            ? "Referenced cell has no rows yet — run it, then this chart updates."
            : "Select a data source above. Name a SQL or SQL-on-FHIR cell and run it to make it selectable."}
        </div>
      )}
    </div>
  );
}
