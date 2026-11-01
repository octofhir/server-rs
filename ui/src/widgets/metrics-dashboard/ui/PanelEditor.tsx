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
import { Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import {
  nextRefId,
  type Panel,
  type PanelTarget,
  type PanelType,
  UNITS,
  type Variables,
} from "@/entities/metrics";
// Imported directly: the metrics entity's index stays free of Monaco for light pages.
import { PromqlEditor } from "@/entities/metrics/promql/PromqlEditor";
import classes from "./PanelEditor.module.css";
import { PanelView } from "./PanelView";

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

const UNIT_OPTIONS = UNITS.map((unit) => ({ value: unit.value, label: unit.label }));

/**
 * Panel editor: PromQL queries with completion and live preview, panel type, unit and the few
 * options each type has. Changes apply to the dashboard only on "Apply".
 */
export function PanelEditor({ panel, onClose, onApply, range, end, variables, variableNames }: PanelEditorProps) {
  const [draft, setDraft] = useState<Panel | null>(panel);
  // The preview runs queries as last applied with ⌘Enter or "Run", not on every keystroke.
  const [preview, setPreview] = useState<Panel | null>(panel);

  useEffect(() => {
    setDraft(panel);
    setPreview(panel);
  }, [panel]);

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
          <div key={target.refId} className={classes.query}>
            <div className={classes.queryHead}>
              <span className={classes.refId}>{target.refId}</span>
              <TextInput
                size="sm"
                value={target.legend ?? ""}
                onChange={(legend) => patchTarget(target.refId, { legend: legend || undefined })}
                placeholder={draft.type === "table" ? "Column title" : "Legend, e.g. {{status_class}}"}
                w={draft.type === "table" ? 180 : 260}
              />
              {draft.type === "table" ? (
                <Select
                  size="sm"
                  data={[{ value: "", label: "Panel unit" }, ...UNIT_OPTIONS]}
                  value={target.unit ?? ""}
                  onChange={(unit) => patchTarget(target.refId, { unit: unit || undefined })}
                  w={150}
                />
              ) : null}
              {draft.targets.length > 1 ? (
                <ActionIcon
                  variant="subtle"
                  size="sm"
                  aria-label={`Remove query ${target.refId}`}
                  onClick={() => patch({ targets: draft.targets.filter((t) => t.refId !== target.refId) })}
                >
                  <Trash2 size={14} />
                </ActionIcon>
              ) : null}
            </div>
            <div className={classes.code}>
              <PromqlEditor
                value={target.expr}
                onChange={(expr) => patchTarget(target.refId, { expr })}
                onRun={run}
                variables={variableNames}
                placeholder="sum(rate(octofhir_http_requests_total[$__rate_interval]))"
                aria-label={`Query ${target.refId}`}
              />
            </div>
          </div>
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
