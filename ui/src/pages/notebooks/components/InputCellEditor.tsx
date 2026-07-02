// Input cell — binds a widget to a notebook variable. Editing the widget writes the
// variable value into the reactive scope, which marks downstream cells stale.

import { Select } from "@octofhir/ui-kit";
import type { InputCell, Variable } from "../model/notebook";
import { variableValueSet } from "../model/store";
import classes from "../NotebookEditor.module.css";
import { VariableWidget } from "./VariableWidget";

interface Props {
  cell: InputCell;
  onChange: (next: InputCell) => void;
  variables: Variable[];
}

export function InputCellEditor({ cell, onChange, variables }: Props) {
  const selected = variables.find((v) => v.name === cell.source.variable);

  return (
    <div className={classes.chartConfig}>
      <span className={classes.fieldLabel}>Bound variable</span>
      <Select
        size="sm"
        data={variables.map((v) => ({ value: v.name, label: `${v.name} (${v.kind})` }))}
        value={cell.source.variable || null}
        onChange={(v) => onChange({ ...cell, source: { variable: v ?? "" } })}
        placeholder={
          variables.length ? "Pick a variable" : "No variables — add one in the Variables panel"
        }
      />
      {selected && (
        <>
          <span className={classes.fieldLabel}>{selected.name}</span>
          <VariableWidget
            variable={selected}
            size="md"
            onValue={(value) => variableValueSet({ name: selected.name, value })}
          />
        </>
      )}
    </div>
  );
}
