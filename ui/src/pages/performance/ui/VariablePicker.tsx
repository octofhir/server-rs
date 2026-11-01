import { Popover } from "@octofhir/ui-kit";
import { Check, ChevronDown, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { type DashboardVariable, useLabelValues } from "@/entities/metrics";
import classes from "./PerformancePage.module.css";

function Tick({ on }: { on: boolean }) {
  return (
    <span className={classes.tick} data-on={on ? "" : undefined} aria-hidden="true">
      {on ? <Check size={11} strokeWidth={3} /> : null}
    </span>
  );
}

export interface VariablePickerProps {
  variable: DashboardVariable;
  /** Selected values; empty means "All" (or nothing for variables without it). */
  value: string[];
  onChange: (value: string[]) => void;
}

/**
 * A dashboard variable as a 28px control: `instance │ All ▾`. The popover lists the label's
 * values with a filter; multi-value variables toggle, single ones pick and close.
 */
export function VariablePicker({ variable, value, onChange }: VariablePickerProps) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const values = useLabelValues(variable.labelName, variable.metric, open || value.length > 0);
  const options = useMemo(() => {
    const all = values.data ?? [];
    const needle = filter.trim().toLowerCase();
    return needle ? all.filter((item) => item.toLowerCase().includes(needle)) : all;
  }, [values.data, filter]);

  const label = variable.label ?? variable.name;
  const summary =
    value.length === 0
      ? variable.includeAll
        ? "All"
        : "—"
      : value.length === 1
        ? value[0]
        : `${value[0]} +${value.length - 1}`;

  const pick = (item: string) => {
    if (!variable.multi) {
      onChange([item]);
      setOpen(false);
      return;
    }
    onChange(value.includes(item) ? value.filter((v) => v !== item) : [...value, item]);
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setFilter("");
      }}
      placement="bottom-start"
      asChild
      content={
        <div className={classes.picker}>
          <label className={classes.pickerSearch}>
            <Search size={13} aria-hidden="true" />
            <input
              autoFocus
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder={`Filter ${label}`}
              aria-label={`Filter ${label}`}
            />
          </label>
          <div className={classes.pickerList} role="listbox" aria-multiselectable={variable.multi}>
            {variable.includeAll ? (
              <button
                type="button"
                role="option"
                aria-selected={value.length === 0}
                className={classes.pickerItem}
                onClick={() => {
                  onChange([]);
                  if (!variable.multi) setOpen(false);
                }}
              >
                <Tick on={value.length === 0} />
                <span>All</span>
              </button>
            ) : null}
            {options.map((item) => (
              <button
                key={item}
                type="button"
                role="option"
                aria-selected={value.includes(item)}
                className={classes.pickerItem}
                onClick={() => pick(item)}
              >
                <Tick on={value.includes(item)} />
                <span className={classes.pickerValue}>{item}</span>
              </button>
            ))}
            {values.isSuccess && options.length === 0 ? <p className={classes.pickerEmpty}>No values</p> : null}
            {values.isError ? <p className={classes.pickerEmpty}>Could not load values</p> : null}
          </div>
        </div>
      }
    >
      <button type="button" className={classes.variable} aria-label={`${label}: ${summary}`}>
        <span className={classes.variableName}>{label}</span>
        <span className={classes.variableValue}>{summary}</span>
        <ChevronDown size={13} aria-hidden="true" />
      </button>
    </Popover>
  );
}
