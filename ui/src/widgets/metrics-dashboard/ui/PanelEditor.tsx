import {
  ActionIcon,
  Button,
  Drawer,
  NumberInput,
  SegmentedRadioGroup,
  Select,
  Switch,
  TextInput,
} from "@octofhir/ui-kit";
import { Code, ListTree, Plus, Trash2, Wand2 } from "lucide-react";
import { useEffect, useState } from "react";
import {
  type BuilderQuery,
  fromPromql,
  nextRefId,
  type Panel,
  type PanelTarget,
  type PanelType,
  toPromql,
  UNITS,
  type Variables,
} from "@/entities/metrics";
// Imported directly: the metrics entity's index stays free of Monaco for light pages.
import { PromqlEditor } from "@/entities/metrics/promql/PromqlEditor";
import classes from "./PanelEditor.module.css";
import { ExplainTree } from "./ExplainTree";
import { PanelView } from "./PanelView";
import { QueryBuilder } from "./QueryBuilder";

export interface PanelEditorProps {
  panel: Panel | null;
  onClose: () => void;
  onApply: (panel: Panel) => void;
  range: number;
  end: number;
  variables: Variables;
  variableNames: string[];
}

const TYPES: { value: PanelType; label: string }[] = [
  { value: "timeseries", label: "Time series" },
  { value: "stat", label: "Stat" },
  { value: "table", label: "Table" },
];

const PREVIEW_DELAY_MS = 700;

const UNIT_OPTIONS = UNITS.map((unit) => ({ value: unit.value, label: unit.label }));

/**
 * One query: legend (or column title and unit for tables), the PromQL editor or the visual
 * builder, and an explain tree on demand. The builder starts open for an empty query; a query the
 * builder cannot read stays in code.
 */
function QueryEditor({
  target,
  tableColumn,
  removable,
  onChange,
  onRemove,
  onRun,
  range,
  end,
  variables,
  variableNames,
}: {
  target: PanelTarget;
  tableColumn: boolean;
  removable: boolean;
  onChange: (change: Partial<PanelTarget>) => void;
  onRemove: () => void;
  onRun: () => void;
  range: number;
  end: number;
  variables: Variables;
  variableNames: string[];
}) {
  const [mode, setMode] = useState<"builder" | "code">(() => (target.expr.trim() === "" ? "builder" : "code"));
  const [builder, setBuilder] = useState<BuilderQuery | null>(() => fromPromql(target.expr));
  const [explaining, setExplaining] = useState(false);
  const [explained, setExplained] = useState(target.expr);
  const readable = builder != null;

  const switchMode = (next: string) => {
    if (next === "builder") {
      const parsed = fromPromql(target.expr);
      if (!parsed) return;
      setBuilder(parsed);
    }
    setMode(next as "builder" | "code");
  };

  return (
    <div className={classes.query}>
      <div className={classes.queryHead}>
        <span className={classes.refId}>{target.refId}</span>
        <TextInput
          size="sm"
          value={target.legend ?? ""}
          onChange={(legend) => onChange({ legend: legend || undefined })}
          placeholder={tableColumn ? "Column title" : "Legend, e.g. {{status_class}}"}
          w={tableColumn ? 170 : 240}
        />
        {tableColumn ? (
          <Select
            size="sm"
            data={[{ value: "", label: "Panel unit" }, ...UNIT_OPTIONS]}
            value={target.unit ?? ""}
            onChange={(unit) => onChange({ unit: unit || undefined })}
            w={140}
          />
        ) : null}
        <span className={classes.queryTools}>
          <SegmentedRadioGroup
            size="sm"
            aria-label={`Query ${target.refId} editor`}
            value={mode}
            onChange={switchMode}
            options={[
              { value: "builder", label: "Builder", disabled: mode === "code" && fromPromql(target.expr) == null },
              { value: "code", label: "Code" },
            ]}
          />
          <ActionIcon
            variant={explaining ? "outline" : "subtle"}
            size="sm"
            aria-label={`Explain query ${target.refId}`}
            title="Explain: evaluate every step of the query"
            aria-pressed={explaining}
            onClick={() => {
              setExplained(target.expr);
              setExplaining((v) => !v);
            }}
          >
            <ListTree size={14} />
          </ActionIcon>
          {removable ? (
            <ActionIcon variant="subtle" size="sm" aria-label={`Remove query ${target.refId}`} onClick={onRemove}>
              <Trash2 size={14} />
            </ActionIcon>
          ) : null}
        </span>
      </div>
      {mode === "builder" && readable ? (
        <QueryBuilder
          query={builder}
          variables={variableNames}
          onChange={(next) => {
            setBuilder(next);
            onChange({ expr: toPromql(next) });
          }}
        />
      ) : (
        <div className={classes.code}>
          <PromqlEditor
            value={target.expr}
            onChange={(expr) => {
              onChange({ expr });
              setBuilder(fromPromql(expr));
            }}
            onRun={() => {
              onRun();
              setExplained(target.expr);
            }}
            variables={variableNames}
            placeholder="sum(rate(octofhir_http_requests_total[$__rate_interval]))"
            aria-label={`Query ${target.refId}`}
          />
        </div>
      )}
      {mode === "builder" ? (
        <span className={classes.modeHint}>
          <Wand2 size={12} aria-hidden="true" /> The builder writes PromQL; switch to <Code size={12} aria-hidden="true" />{" "}
          Code for anything beyond it.
        </span>
      ) : null}
      {explaining ? (
        <div className={classes.explain}>
          <div className={classes.explainHead}>
            <span>Explain</span>
            <button type="button" className={classes.linkButton} onClick={() => setExplained(target.expr)}>
              Re-run
            </button>
          </div>
          <ExplainTree query={explained} end={end} range={range} variables={variables} />
        </div>
      ) : null}
    </div>
  );
}

/**
 * Panel editor: PromQL queries with completion and live preview, panel type, unit and the few
 * options each type has. Changes apply to the dashboard only on "Apply".
 */
export function PanelEditor({ panel, onClose, onApply, range, end, variables, variableNames }: PanelEditorProps) {
  const [draft, setDraft] = useState<Panel | null>(panel);
  const [preview, setPreview] = useState<Panel | null>(panel);

  useEffect(() => {
    setDraft(panel);
    setPreview(panel);
  }, [panel]);

  // The preview follows edits once they pause, so a builder change or a typed query shows up
  // without pressing Run (which applies immediately).
  useEffect(() => {
    const timer = window.setTimeout(() => setPreview(draft), PREVIEW_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [draft]);

  if (!draft) return null;

  const patch = (change: Partial<Panel>) => setDraft((current) => (current ? { ...current, ...change } : current));
  const patchTarget = (refId: string, change: Partial<PanelTarget>) =>
    patch({ targets: draft.targets.map((target) => (target.refId === refId ? { ...target, ...change } : target)) });
  const run = () => setPreview(draft);

  const warning = draft.thresholds?.find((t) => t.color === "warning")?.value ?? null;
  const critical = draft.thresholds?.find((t) => t.color === "error")?.value ?? null;
  const setThresholds = (next: { warning?: number | null; error?: number | null }) => {
    const w = next.warning !== undefined ? next.warning : warning;
    const e = next.error !== undefined ? next.error : critical;
    patch({
      thresholds: [
        ...(w != null ? [{ value: w, color: "warning" as const }] : []),
        ...(e != null ? [{ value: e, color: "error" as const }] : []),
      ],
    });
  };

  if (draft.type === "row") {
    return (
      <Drawer open onClose={onClose} placement="right" size="min(420px, 100vw)" title="Section">
        <form
          className={classes.editor}
          onSubmit={(event) => {
            event.preventDefault();
            if (draft.title.trim()) onApply(draft);
          }}
        >
          <TextInput label="Title" value={draft.title} onChange={(title) => patch({ title })} autoFocus w="100%" />
          <footer className={classes.footer}>
            <Button variant="subtle" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" color="primary" disabled={!draft.title.trim()}>
              Apply
            </Button>
          </footer>
        </form>
      </Drawer>
    );
  }

  return (
    <Drawer
      open
      onClose={onClose}
      placement="right"
      size="min(760px, 100vw)"
      title={draft.title || "Panel"}
    >
      <div className={classes.editor}>
        <div className={classes.preview}>
          {preview ? <PanelView panel={{ ...preview, title: draft.title }} range={range} end={end} variables={variables} /> : null}
        </div>

        <div className={classes.row}>
          <TextInput label="Title" value={draft.title} onChange={(title) => patch({ title })} w="100%" />
        </div>
        <div className={classes.row}>
          <TextInput
            label="Description"
            value={draft.description ?? ""}
            onChange={(description) => patch({ description: description || undefined })}
            placeholder="Shown on hover"
            w="100%"
          />
        </div>
        <div className={classes.row}>
          <div className={classes.field}>
            <span className={classes.label}>Type</span>
            <SegmentedRadioGroup
              options={TYPES}
              value={draft.type}
              onChange={(type) => {
                const next = { ...draft, type: type as PanelType };
                setDraft(next);
                setPreview(next);
              }}
            />
          </div>
          <Select
            label="Unit"
            data={UNIT_OPTIONS}
            value={draft.unit ?? "short"}
            onChange={(unit) => patch({ unit: unit ?? undefined })}
            w={180}
          />
          <NumberInput
            label="Decimals"
            value={draft.decimals ?? null}
            onChange={(decimals) => patch({ decimals: decimals ?? undefined })}
            min={0}
            max={6}
            placeholder="auto"
            w={120}
          />
        </div>

        {draft.type === "timeseries" ? (
          <div className={classes.row}>
            <div className={classes.field}>
              <span className={classes.label}>Draw</span>
              <SegmentedRadioGroup
                options={[
                  { value: "line", label: "Lines" },
                  { value: "bars", label: "Bars" },
                ]}
                value={draft.drawStyle ?? "line"}
                onChange={(drawStyle) => {
                  const next = { ...draft, drawStyle: drawStyle as Panel["drawStyle"] };
                  setDraft(next);
                  setPreview(next);
                }}
              />
            </div>
            <Switch
              label="Stack series"
              checked={draft.stack ?? false}
              onChange={(stack) => {
                const next = { ...draft, stack };
                setDraft(next);
                setPreview(next);
              }}
            />
          </div>
        ) : null}

        {draft.type === "stat" ? (
          <div className={classes.row}>
            <NumberInput
              label="Warning at"
              value={warning}
              onChange={(value) => setThresholds({ warning: value })}
              placeholder="none"
              step={0.01}
              w={140}
            />
            <NumberInput
              label="Error at"
              value={critical}
              onChange={(value) => setThresholds({ error: value })}
              placeholder="none"
              step={0.01}
              w={140}
            />
          </div>
        ) : null}

        <div className={classes.queriesHeader}>
          <span className={classes.label}>Queries</span>
          <span className={classes.hint}>⌘Enter runs · $__rate_interval, $__range{variableNames.map((n) => `, $${n}`).join("")}</span>
        </div>
        {draft.targets.map((target) => (
          <QueryEditor
            key={target.refId}
            target={target}
            tableColumn={draft.type === "table"}
            removable={draft.targets.length > 1}
            onChange={(change) => patchTarget(target.refId, change)}
            onRemove={() => patch({ targets: draft.targets.filter((t) => t.refId !== target.refId) })}
            onRun={run}
            range={range}
            end={end}
            variables={variables}
            variableNames={variableNames}
          />
        ))}
        <div className={classes.row}>
          <Button
            variant="outline"
            size="sm"
            onClick={() =>
              patch({ targets: [...draft.targets, { refId: nextRefId(draft.targets), expr: "" }] })
            }
            disabled={draft.targets.length >= 10}
          >
            <Button.Icon side="left">
              <Plus size={14} />
            </Button.Icon>
            Add query
          </Button>
          <Button variant="outline" size="sm" onClick={run}>
            Run
          </Button>
        </div>

        <footer className={classes.footer}>
          <Button variant="subtle" onClick={onClose}>
            Cancel
          </Button>
          <Button color="primary" onClick={() => onApply(draft)} disabled={!draft.title.trim()}>
            Apply
          </Button>
        </footer>
      </div>
    </Drawer>
  );
}
