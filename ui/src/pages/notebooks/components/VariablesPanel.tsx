// Variables drawer — list / add / edit / remove notebook-level reactive variables.
// Editing a value flows into $scope and marks the downstream DAG stale.

import { ActionIcon, Button, Drawer, Select, TextInput } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { Plus, Trash2 } from "lucide-react";
import type { Variable, VarKind } from "../model/notebook";
import {
  $variables,
  variableAdded,
  variableRemoved,
  variableUpdated,
  variableValueSet,
} from "../model/store";
import classes from "../NotebookEditor.module.css";
import { VariableWidget } from "./VariableWidget";

const KIND_OPTIONS: { value: VarKind; label: string }[] = [
  { value: "string", label: "string" },
  { value: "integer", label: "integer" },
  { value: "decimal", label: "decimal" },
  { value: "boolean", label: "boolean" },
  { value: "date", label: "date" },
  { value: "dateTime", label: "dateTime" },
  { value: "code", label: "code" },
  { value: "json", label: "json" },
];

const WIDGET_OPTIONS = ["text", "number", "select", "switch", "date", "none"].map((w) => ({
  value: w,
  label: w,
}));

function nextName(vars: Variable[]): string {
  let i = vars.length + 1;
  while (vars.some((v) => v.name === `var${i}`)) i += 1;
  return `var${i}`;
}

interface Props {
  opened: boolean;
  onClose: () => void;
}

export function VariablesPanel({ opened, onClose }: Props) {
  const variables = useUnit($variables);

  return (
    <Drawer opened={opened} onClose={onClose} title="Variables & inputs" size={420}>
      <div className={classes.varsPanel}>
        {variables.length === 0 && (
          <div className={classes.emptyHint}>
            No variables yet. Add one, then reference it as{" "}
            <code>
              {"$"}
              {"{name}"}
            </code>{" "}
            in any cell.
          </div>
        )}

        {variables.map((v, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: variables are positional
          <div key={i} className={classes.varRow}>
            <div className={classes.varRowHead}>
              <TextInput
                size="sm"
                value={v.name}
                onChange={(name) => variableUpdated({ index: i, next: { ...v, name } })}
                placeholder="name"
                className={classes.varName}
              />
              <Select
                size="sm"
                data={KIND_OPTIONS}
                value={v.kind}
                onChange={(kind) =>
                  kind && variableUpdated({ index: i, next: { ...v, kind: kind as VarKind } })
                }
                className={classes.varKind}
              />
              <ActionIcon variant="subtle" aria-label="Remove" onClick={() => variableRemoved(i)}>
                <Trash2 size={15} />
              </ActionIcon>
            </div>
            <div className={classes.varRowBody}>
              <Select
                size="sm"
                data={WIDGET_OPTIONS}
                value={v.widget ?? "none"}
                onChange={(widget) =>
                  variableUpdated({
                    index: i,
                    next: { ...v, widget: widget as Variable["widget"] },
                  })
                }
                className={classes.varWidgetSel}
              />
              <VariableWidget
                variable={v}
                onValue={(value) => variableValueSet({ name: v.name, value })}
              />
            </div>
          </div>
        ))}

        <Button
          variant="subtle"
          leftSection={<Plus size={15} />}
          onClick={() =>
            variableAdded({ name: nextName(variables), kind: "string", value: "", widget: "text" })
          }
        >
          Add variable
        </Button>
      </div>
    </Drawer>
  );
}
