import { Badge, Button, IconPlus, Select, Tabs, Text, TextInput } from "@octofhir/ui-kit";
import { Braces, Filter, Sigma, Table2 } from "lucide-react";
import { useCallback, useMemo } from "react";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import type { ViewDefinition, ViewDefinitionSelect } from "../../lib/useViewDefinition";
import { ConstantsEditor } from "./ConstantsEditor";
import classes from "./EditorPanel.module.css";
import { SelectNode } from "./SelectNode";
import { WhereClauseEditor } from "./WhereClauseEditor";

type ViewDefinitionStatus = ViewDefinition["status"];

const STATUS_OPTIONS = [
  { value: "draft", label: "Draft" },
  { value: "active", label: "Active" },
  { value: "retired", label: "Retired" },
  { value: "unknown", label: "Unknown" },
] satisfies Array<{ value: ViewDefinitionStatus; label: string }>;

function isStatus(value: string | null | undefined): value is ViewDefinitionStatus {
  return STATUS_OPTIONS.some((o) => o.value === value);
}

const uid = () => crypto.randomUUID();

interface EditorPanelProps {
  value: ViewDefinition;
  resourceTypes: string[];
  /** Sampled values keyed by column name, from the first preview row. */
  sampleRow?: Record<string, string>;
  onChange: (viewDef: ViewDefinition) => void;
}

export function EditorPanel({
  value: viewDef,
  resourceTypes,
  sampleRow,
  onChange,
}: EditorPanelProps) {
  const selects = viewDef.select.length > 0 ? viewDef.select : [{ column: [], _id: uid() }];

  const changeSelect = useCallback(
    (index: number, node: ViewDefinitionSelect) =>
      onChange({ ...viewDef, select: selects.map((s, i) => (i === index ? node : s)) }),
    [onChange, selects, viewDef]
  );
  const removeSelect = useCallback(
    (index: number) => onChange({ ...viewDef, select: selects.filter((_, i) => i !== index) }),
    [onChange, selects, viewDef]
  );
  const addSelect = useCallback(
    () =>
      onChange({
        ...viewDef,
        select: [...selects, { column: [{ name: "", path: "", _id: uid() }], _id: uid() }],
      }),
    [onChange, selects, viewDef]
  );

  const jsonText = useMemo(() => JSON.stringify(stripIds(viewDef), null, 2), [viewDef]);
  const handleJsonChange = useCallback(
    (text: string) => {
      try {
        const parsed = JSON.parse(text) as ViewDefinition;
        if (parsed && parsed.resourceType === "ViewDefinition") {
          onChange(parsed);
        }
      } catch {
        // ignore invalid JSON while typing
      }
    },
    [onChange]
  );

  const whereCount = viewDef.where?.length ?? 0;
  const constCount = viewDef.constant?.length ?? 0;

  return (
    <div className={classes.panel}>
      <Tabs defaultValue="visual" className={classes.tabs}>
        <Tabs.List>
          <Tabs.Tab value="visual">
            <Table2 size={13} /> Visual
          </Tabs.Tab>
          <Tabs.Tab value="json">
            <Braces size={13} /> JSON
          </Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="visual" className={classes.tabPanel}>
          <div className={classes.scroll}>
            {/* Meta */}
            <div className={classes.meta}>
              <TextInput
                label="Name"
                placeholder="my_patient_view"
                value={viewDef.name}
                onChange={(v) => onChange({ ...viewDef, name: v })}
                className={classes.metaName}
                size="xs"
              />
              <Select
                label="Resource"
                placeholder="Resource type"
                value={viewDef.resource}
                onUpdate={(v) => onChange({ ...viewDef, resource: v || "Patient" })}
                data={resourceTypes}
                searchable
                size="xs"
                className={classes.metaResource}
              />
              <Select
                label="Status"
                value={viewDef.status}
                onUpdate={(v) => onChange({ ...viewDef, status: isStatus(v) ? v : "draft" })}
                data={STATUS_OPTIONS}
                size="xs"
                className={classes.metaStatus}
              />
            </div>

            {/* Projection (recursive select tree) */}
            <section className={classes.section}>
              <header className={classes.sectionHead}>
                <span className={classes.sectionTitle}>
                  <Sigma size={14} />
                  <Text size="sm" fw={600}>
                    Projection
                  </Text>
                </span>
                <Button
                  variant="subtle"
                  size="xs"
                  leftSection={<IconPlus size={12} />}
                  onClick={addSelect}
                >
                  Top-level select
                </Button>
              </header>
              {selects.map((node, i) => (
                <SelectNode
                  key={node._id || i}
                  value={node}
                  onChange={(n) => changeSelect(i, n)}
                  onRemove={selects.length > 1 ? () => removeSelect(i) : undefined}
                  resourceType={viewDef.resource}
                  constants={viewDef.constant}
                  sampleRow={sampleRow}
                  depth={0}
                  variant={selects.length === 1 ? "root" : "select"}
                  label={`Select ${i + 1}`}
                />
              ))}
            </section>

            {/* Where */}
            <section className={classes.section}>
              <header className={classes.sectionHead}>
                <span className={classes.sectionTitle}>
                  <Filter size={14} />
                  <Text size="sm" fw={600}>
                    Where
                  </Text>
                  {whereCount > 0 && (
                    <Badge size="xs" variant="light">
                      {whereCount}
                    </Badge>
                  )}
                </span>
              </header>
              <WhereClauseEditor
                whereClauses={viewDef.where || []}
                resourceType={viewDef.resource}
                constants={viewDef.constant}
                onChange={(where) => onChange({ ...viewDef, where })}
              />
            </section>

            {/* Constants */}
            <section className={classes.section}>
              <header className={classes.sectionHead}>
                <span className={classes.sectionTitle}>
                  <Sigma size={14} />
                  <Text size="sm" fw={600}>
                    Constants
                  </Text>
                  {constCount > 0 && (
                    <Badge size="xs" variant="light">
                      {constCount}
                    </Badge>
                  )}
                </span>
              </header>
              <ConstantsEditor
                constants={viewDef.constant || []}
                onChange={(constant) => onChange({ ...viewDef, constant })}
              />
            </section>
          </div>
        </Tabs.Panel>

        <Tabs.Panel value="json" className={classes.tabPanel}>
          <JsonEditor value={jsonText} onChange={handleJsonChange} height="100%" />
        </Tabs.Panel>
      </Tabs>
    </div>
  );
}

/** Strip editor-internal `_id` keys for the JSON view. */
function stripIds<T>(value: T): T {
  if (Array.isArray(value)) return value.map(stripIds) as unknown as T;
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      if (k === "_id") continue;
      out[k] = stripIds(v);
    }
    return out as T;
  }
  return value;
}
