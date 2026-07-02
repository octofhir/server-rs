// Pipeline cell editor — pick a named table, chain no-code transform steps.
// Client engine runs on execution; output is a normal table (chartable/referenceable).

import { ActionIcon, Menu, NumberInput, Select, TextInput } from "@octofhir/ui-kit";
import { ArrowDown, ArrowUp, Plus, X } from "lucide-react";
import type { Agg, PipelineCell, Scope, Step, StepOp } from "../model/notebook";
import classes from "../NotebookEditor.module.css";

interface Props {
  cell: PipelineCell;
  onChange: (next: PipelineCell) => void;
  namedCells: { id: string; name: string; label: string }[];
  scope: Scope;
}

const STEP_OPS: { op: StepOp; label: string }[] = [
  { op: "filter", label: "Filter rows" },
  { op: "select", label: "Select columns" },
  { op: "rename", label: "Rename columns" },
  { op: "derive", label: "Derive column" },
  { op: "groupBy", label: "Group & aggregate" },
  { op: "sort", label: "Sort" },
  { op: "limit", label: "Limit" },
  { op: "distinct", label: "Distinct" },
];

function blankStep(op: StepOp): Step {
  switch (op) {
    case "filter":
      return { op, where: "" };
    case "select":
      return { op, columns: [] };
    case "rename":
      return { op, map: {} };
    case "derive":
      return { op, as: "", expr: "" };
    case "groupBy":
      return { op, keys: [], agg: [{ fn: "count", as: "n" }] };
    case "sort":
      return { op, by: "", dir: "asc" };
    case "limit":
      return { op, n: 100 };
    default:
      return { op: "distinct", columns: [] };
  }
}

const csv = (s: string): string[] =>
  s
    .split(",")
    .map((x) => x.trim())
    .filter(Boolean);

/** Parse "count:n, sum(age):total, avg(age):mean" → Agg[]. */
function parseAggs(s: string): Agg[] {
  return csv(s).map((part) => {
    const [left, as] = part.split(":").map((x) => x.trim());
    const m = left.match(/^(\w+)\s*(?:\(([^)]*)\))?$/);
    const fn = (m?.[1] ?? "count") as Agg["fn"];
    const col = m?.[2]?.trim() || undefined;
    return { fn, col, as: as || `${fn}${col ? `_${col}` : ""}` };
  });
}
const aggsToStr = (aggs: Agg[]): string =>
  aggs.map((a) => `${a.fn}${a.col ? `(${a.col})` : ""}:${a.as}`).join(", ");

export function PipelineCellEditor({ cell, onChange, namedCells, scope }: Props) {
  const { input, steps } = cell.source;
  const inputData = scope[input] as { columns?: string[]; rows?: unknown[][] } | undefined;
  const cols = inputData?.columns ?? [];
  const rows = inputData?.rows ?? [];

  const setSteps = (next: Step[]) => onChange({ ...cell, source: { ...cell.source, steps: next } });
  const patch = (i: number, next: Step) => setSteps(steps.map((s, j) => (j === i ? next : s)));
  const remove = (i: number) => setSteps(steps.filter((_s, j) => j !== i));
  const move = (i: number, dir: -1 | 1) => {
    const j = i + dir;
    if (j < 0 || j >= steps.length) return;
    const next = [...steps];
    [next[i], next[j]] = [next[j], next[i]];
    setSteps(next);
  };

  return (
    <div className={classes.pipeEditor}>
      <div className={classes.chartInputRow}>
        <span className={classes.ctxLabel}>Input</span>
        <Select
          data={namedCells.map((c) => ({ value: c.name, label: c.label }))}
          value={input || null}
          onChange={(v) => onChange({ ...cell, source: { ...cell.source, input: v ?? "" } })}
          placeholder={
            namedCells.length ? "Pick a named table cell" : "Name & run a table cell first"
          }
          size="sm"
          searchable
          className={classes.chartInputSel}
        />
      </div>

      {namedCells.length === 0 ? (
        <div className={classes.pipeGuide}>
          Pipelines transform a table produced by another cell. Add a <b>SQL</b> or{" "}
          <b>SQL-on-FHIR</b> cell above, give it a <b>name</b> (top field), <b>run</b> it (▶) — then
          select it here.
        </div>
      ) : !input ? (
        <div className={classes.pipeGuide}>Select a source table above to start transforming.</div>
      ) : cols.length === 0 ? (
        <div className={classes.pipeGuide}>
          "{input}" has no rows yet — run that cell, then its columns appear here.
        </div>
      ) : (
        <div className={classes.pipePreview}>
          <div className={classes.pipePreviewHead}>
            {rows.length} row{rows.length === 1 ? "" : "s"} · {cols.length} columns · click a column
            to copy its name
          </div>
          <div className={classes.pipeChips}>
            {cols.map((c) => (
              <button
                key={c}
                type="button"
                className={classes.pipeChip}
                title="Copy column name"
                onClick={() => navigator.clipboard?.writeText(c)}
              >
                {c}
              </button>
            ))}
          </div>
          <div className={classes.pipeMiniWrap}>
            <table className={classes.pipeMini}>
              <thead>
                <tr>
                  {cols.map((c) => (
                    <th key={c}>{c}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.slice(0, 5).map((row, ri) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: positional preview rows
                  <tr key={ri}>
                    {cols.map((_c, ci) => {
                      const v = Array.isArray(row) ? row[ci] : undefined;
                      return (
                        <td key={cols[ci]}>
                          {v == null ? "∅" : typeof v === "object" ? JSON.stringify(v) : String(v)}
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {steps.map((step, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: steps are positional
        <div key={i} className={classes.pipeStep}>
          <span className={classes.pipeOp}>{step.op}</span>
          <div className={classes.pipeFields}>
            {step.op === "filter" && (
              <TextInput
                value={step.where}
                onChange={(where) => patch(i, { ...step, where })}
                placeholder="age >= 40 && gender === 'male'"
                size="sm"
              />
            )}
            {step.op === "select" && (
              <TextInput
                value={step.columns.join(", ")}
                onChange={(v) => patch(i, { ...step, columns: csv(v) })}
                placeholder="id, gender, age"
                size="sm"
              />
            )}
            {step.op === "rename" && (
              <TextInput
                value={Object.entries(step.map)
                  .map(([k, v]) => `${k}:${v}`)
                  .join(", ")}
                onChange={(v) => {
                  const map: Record<string, string> = {};
                  for (const part of csv(v)) {
                    const [k, nv] = part.split(":").map((x) => x.trim());
                    if (k && nv) map[k] = nv;
                  }
                  patch(i, { ...step, map });
                }}
                placeholder="old:new, n:count"
                size="sm"
              />
            )}
            {step.op === "derive" && (
              <div className={classes.pipeTwo}>
                <TextInput
                  value={step.as}
                  onChange={(as) => patch(i, { ...step, as })}
                  placeholder="new column"
                  size="sm"
                />
                <TextInput
                  value={step.expr}
                  onChange={(expr) => patch(i, { ...step, expr })}
                  placeholder="Math.floor(age/10)*10"
                  size="sm"
                />
              </div>
            )}
            {step.op === "groupBy" && (
              <div className={classes.pipeTwo}>
                <TextInput
                  value={step.keys.join(", ")}
                  onChange={(v) => patch(i, { ...step, keys: csv(v) })}
                  placeholder="keys: gender"
                  size="sm"
                />
                <TextInput
                  value={aggsToStr(step.agg)}
                  onChange={(v) => patch(i, { ...step, agg: parseAggs(v) })}
                  placeholder="count:n, avg(age):meanAge"
                  size="sm"
                />
              </div>
            )}
            {step.op === "sort" && (
              <div className={classes.pipeTwo}>
                <TextInput
                  value={step.by}
                  onChange={(by) => patch(i, { ...step, by })}
                  placeholder="column"
                  size="sm"
                />
                <Select
                  data={[
                    { value: "asc", label: "asc" },
                    { value: "desc", label: "desc" },
                  ]}
                  value={step.dir ?? "asc"}
                  onChange={(d) => patch(i, { ...step, dir: (d ?? "asc") as "asc" | "desc" })}
                  size="sm"
                />
              </div>
            )}
            {step.op === "limit" && (
              <NumberInput
                value={step.n}
                onChange={(n) => patch(i, { ...step, n: n ?? 0 })}
                size="sm"
              />
            )}
            {step.op === "distinct" && (
              <TextInput
                value={(step.columns ?? []).join(", ")}
                onChange={(v) => patch(i, { ...step, columns: csv(v) })}
                placeholder="all columns (or list a subset)"
                size="sm"
              />
            )}
          </div>
          <div className={classes.pipeStepActions}>
            <ActionIcon variant="subtle" size="sm" disabled={i === 0} onClick={() => move(i, -1)}>
              <ArrowUp size={14} />
            </ActionIcon>
            <ActionIcon
              variant="subtle"
              size="sm"
              disabled={i === steps.length - 1}
              onClick={() => move(i, 1)}
            >
              <ArrowDown size={14} />
            </ActionIcon>
            <ActionIcon variant="subtle" size="sm" onClick={() => remove(i)}>
              <X size={14} />
            </ActionIcon>
          </div>
        </div>
      ))}

      <Menu position="bottom-start">
        <Menu.Target>
          <button type="button" className={classes.pipeAdd}>
            <Plus size={14} /> Add step
          </button>
        </Menu.Target>
        <Menu.Dropdown>
          {STEP_OPS.map((o) => (
            <Menu.Item key={o.op} onClick={() => setSteps([...steps, blankStep(o.op)])}>
              {o.label}
            </Menu.Item>
          ))}
        </Menu.Dropdown>
      </Menu>
    </div>
  );
}
