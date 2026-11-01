import { ActionIcon, Autocomplete, Button, MultiSelect, NumberInput, Select } from "@octofhir/ui-kit";
import { Plus, X } from "lucide-react";
import { useMemo } from "react";
import {
  type Aggregation,
  type BuilderFunction,
  type BuilderQuery,
  builderFor,
  type LabelFilter,
  type MatchOp,
  toPromql,
  useLabelValues,
  useMetricCatalog,
  useMetricLabels,
} from "@/entities/metrics";
import classes from "./PanelEditor.module.css";

const FUNCTIONS: { value: BuilderFunction; label: string }[] = [
  { value: "none", label: "Raw value" },
  { value: "rate", label: "rate — per second" },
  { value: "irate", label: "irate — instant rate" },
  { value: "increase", label: "increase — total over range" },
  { value: "histogram_quantile", label: "histogram quantile" },
  { value: "avg_over_time", label: "avg_over_time" },
  { value: "max_over_time", label: "max_over_time" },
  { value: "min_over_time", label: "min_over_time" },
];

const AGGREGATIONS: { value: Aggregation; label: string }[] = [
  { value: "none", label: "No aggregation" },
  { value: "sum", label: "sum" },
  { value: "avg", label: "avg" },
  { value: "max", label: "max" },
  { value: "min", label: "min" },
  { value: "count", label: "count" },
];

const RANGES = ["$__rate_interval", "$__range", "$__interval", "1m", "5m", "15m", "1h"];
const OPS: MatchOp[] = ["=", "!=", "=~", "!~"];

function FilterRow({
  filter,
  metric,
  labels,
  variables,
  onChange,
  onRemove,
}: {
  filter: LabelFilter;
  metric: string;
  labels: string[];
  variables: string[];
  onChange: (filter: LabelFilter) => void;
  onRemove: () => void;
}) {
  const values = useLabelValues(filter.label, metric, filter.label !== "");
  const valueOptions = [...variables.map((v) => `$${v}`), ...(values.data ?? [])];
  return (
    <div className={classes.filterRow}>
      <Select
        aria-label="Label"
        size="sm"
        w={170}
        value={filter.label || null}
        onChange={(label) => onChange({ ...filter, label: label ?? "" })}
        data={labels.map((label) => ({ value: label, label }))}
        placeholder="label"
        searchable
      />
      <Select
        aria-label="Operator"
        size="sm"
        w={64}
        value={filter.op}
        onChange={(op) => onChange({ ...filter, op: (op ?? "=") as MatchOp })}
        data={OPS.map((op) => ({ value: op, label: op }))}
      />
      <Autocomplete
        aria-label="Value"
        size="sm"
        value={filter.value}
        onChange={(value) => onChange({ ...filter, value })}
        data={valueOptions}
        placeholder={filter.op.includes("~") ? "regex" : "value"}
      />
      <ActionIcon variant="subtle" size="sm" aria-label="Remove filter" onClick={onRemove}>
        <X size={14} />
      </ActionIcon>
    </div>
  );
}

/**
 * Builds a PromQL query from a metric, label filters, a function and an aggregation. Every
 * change renders straight to PromQL, which stays the stored form.
 */
export function QueryBuilder({
  query,
  onChange,
  variables,
}: {
  query: BuilderQuery;
  onChange: (query: BuilderQuery) => void;
  variables: string[];
}) {
  const catalog = useMetricCatalog();
  const labels = useMetricLabels(query.metric || undefined, query.metric !== "");
  const labelNames = useMemo(() => (labels.data ?? []).filter((l) => l !== "__name__"), [labels.data]);
  const metricOptions = useMemo(
    () => (catalog.data ?? []).map((m) => ({ value: m.name, label: m.name })),
    [catalog.data]
  );
  const help = catalog.data?.find((m) => m.name === query.metric.replace(/_bucket$/, ""))?.help;
  const rangeFn = query.fn !== "none";
  const patch = (change: Partial<BuilderQuery>) => onChange({ ...query, ...change });

  return (
    <div className={classes.builder}>
      <div className={classes.builderRow}>
        <span className={classes.builderLabel}>Metric</span>
        <Select
          aria-label="Metric"
          size="sm"
          w={360}
          value={query.metric || null}
          onChange={(metric) => {
            if (!metric) return;
            const type = catalog.data?.find((m) => m.name === metric)?.type;
            onChange({ ...builderFor(metric, type), filters: query.filters });
          }}
          data={metricOptions}
          placeholder={catalog.isLoading ? "Loading metrics…" : "Choose a metric"}
          searchable
        />
        {help ? <span className={classes.builderHelp}>{help}</span> : null}
      </div>

      <div className={classes.builderRow}>
        <span className={classes.builderLabel}>Filters</span>
        <div className={classes.filters}>
          {query.filters.map((filter, index) => (
            <FilterRow
              key={index}
              filter={filter}
              metric={query.metric}
              labels={labelNames}
              variables={variables}
              onChange={(next) => patch({ filters: query.filters.map((f, i) => (i === index ? next : f)) })}
              onRemove={() => patch({ filters: query.filters.filter((_, i) => i !== index) })}
            />
          ))}
          <Button
            variant="subtle"
            size="sm"
            disabled={!query.metric}
            onClick={() => patch({ filters: [...query.filters, { label: "", op: "=", value: "" }] })}
          >
            <Button.Icon side="left">
              <Plus size={14} />
            </Button.Icon>
            Filter
          </Button>
        </div>
      </div>

      <div className={classes.builderRow}>
        <span className={classes.builderLabel}>Function</span>
        <Select
          aria-label="Function"
          size="sm"
          w={240}
          value={query.fn}
          onChange={(fn) => patch({ fn: (fn ?? "none") as BuilderFunction })}
          data={FUNCTIONS}
        />
        {rangeFn ? (
          <Select
            aria-label="Range"
            size="sm"
            w={170}
            value={query.range}
            onChange={(range) => patch({ range: range ?? "$__rate_interval" })}
            data={RANGES.map((r) => ({ value: r, label: r }))}
          />
        ) : null}
        {query.fn === "histogram_quantile" ? (
          <Select
            aria-label="Quantile"
            size="sm"
            w={96}
            value={String(query.quantile)}
            onChange={(value) => patch({ quantile: Number(value ?? 0.95) })}
            data={["0.5", "0.9", "0.95", "0.99"].map((v) => ({ value: v, label: `p${Number(v) * 100}` }))}
          />
        ) : null}
      </div>

      <div className={classes.builderRow}>
        <span className={classes.builderLabel}>{query.fn === "histogram_quantile" ? "Split by" : "Aggregate"}</span>
        {query.fn !== "histogram_quantile" ? (
          <Select
            aria-label="Aggregation"
            size="sm"
            w={170}
            value={query.aggregation}
            onChange={(aggregation) => patch({ aggregation: (aggregation ?? "none") as Aggregation })}
            data={AGGREGATIONS}
          />
        ) : null}
        {query.aggregation !== "none" || query.fn === "histogram_quantile" ? (
          <MultiSelect
            aria-label="Group by labels"
            size="sm"
            w={300}
            value={query.by}
            onChange={(by) => patch({ by })}
            data={labelNames.filter((l) => l !== "le")}
            placeholder="by labels"
            searchable
          />
        ) : null}
        <NumberInput
          aria-label="Top k"
          size="sm"
          w={110}
          min={0}
          max={100}
          value={query.topk || null}
          onChange={(topk) => patch({ topk: topk ?? 0 })}
          placeholder="top k"
        />
      </div>

      <code className={classes.builderOutput}>{toPromql(query) || "—"}</code>
    </div>
  );
}
