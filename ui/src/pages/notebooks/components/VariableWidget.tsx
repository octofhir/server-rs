// Renders the right input control for a notebook Variable, driven by its `widget`
// (falling back to `kind`). Shared by the Variables panel and the input cell.

import { DatePicker, NumberInput, Select, Switch, TextInput } from "@octofhir/ui-kit";
import type { Variable } from "../model/notebook";

interface Props {
  variable: Variable;
  onValue: (value: unknown) => void;
  size?: "sm" | "md";
}

function widgetFor(v: Variable): NonNullable<Variable["widget"]> {
  if (v.widget && v.widget !== "none") return v.widget;
  switch (v.kind) {
    case "integer":
    case "decimal":
      return "number";
    case "boolean":
      return "switch";
    case "date":
    case "dateTime":
      return "date";
    default:
      return "text";
  }
}

export function VariableWidget({ variable, onValue, size = "sm" }: Props) {
  const widget = widgetFor(variable);

  if (variable.options?.length || variable.widget === "select") {
    return (
      <Select
        size={size}
        data={(variable.options ?? []).map((o) => ({
          value: String(o.value),
          label: o.label,
        }))}
        value={variable.value == null ? null : String(variable.value)}
        onChange={(v) => onValue(v)}
        placeholder="Select…"
      />
    );
  }

  switch (widget) {
    case "number":
      return (
        <NumberInput
          size={size}
          value={typeof variable.value === "number" ? variable.value : null}
          onChange={(v) => onValue(v)}
        />
      );
    case "switch":
      return <Switch size={size} checked={Boolean(variable.value)} onChange={(v) => onValue(v)} />;
    case "date":
      return (
        <DatePicker
          size={size}
          value={variable.value ? new Date(String(variable.value)) : null}
          onChange={(d) => onValue(d ? d.toISOString().slice(0, 10) : null)}
        />
      );
    default:
      return (
        <TextInput
          size={size}
          value={variable.value == null ? "" : String(variable.value)}
          onChange={(v) => onValue(v)}
        />
      );
  }
}
