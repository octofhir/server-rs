import { useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";
import { type ExplainNode, MetricsApiError, metricsApi, type Variables } from "@/entities/metrics";
import classes from "./PanelEditor.module.css";

const KIND_LABELS: Record<ExplainNode["kind"], string> = {
  aggregation: "agg",
  binary: "op",
  function: "fn",
  selector: "series",
  "range-selector": "range",
  subquery: "sub",
  unary: "op",
  number: "num",
  string: "str",
  extension: "ext",
};

function sampleText(sample: unknown): string {
  if (Array.isArray(sample)) return String(sample[1] ?? "");
  if (sample && typeof sample === "object") {
    const s = sample as { metric?: Record<string, string>; value?: unknown[]; values?: unknown[][] };
    const labels = Object.entries(s.metric ?? {})
      .map(([k, v]) => `${k}="${v}"`)
      .join(", ");
    const value = s.value ? String(s.value[1]) : s.values ? `${s.values.length} points` : "";
    return `{${labels}} ${value}`;
  }
  return String(sample);
}

function summary(node: ExplainNode): { text: string; tone?: "error" | "empty" } {
  const result = node.result;
  if (!result) return { text: node.kind === "number" || node.kind === "string" ? "" : "not evaluated" };
  if (result.error) return { text: result.error, tone: "error" };
  if (result.resultType === "scalar") return { text: `scalar ${sampleText(result.samples[0])}` };
  if (result.series === 0) return { text: "no series", tone: "empty" };
  return { text: `${result.series} series` };
}

function Node({ node, depth }: { node: ExplainNode; depth: number }) {
  const [open, setOpen] = useState(false);
  const { text, tone } = summary(node);
  const samples = node.result?.samples ?? [];
  return (
    <li>
      <button
        type="button"
        className={classes.explainRow}
        style={{ paddingLeft: 8 + depth * 16 }}
        onClick={() => setOpen((v) => !v)}
        title={node.expr}
        aria-expanded={samples.length > 0 ? open : undefined}
      >
        <span className={classes.explainChevron} aria-hidden="true">
          {samples.length > 0 ? open ? <ChevronDown size={12} /> : <ChevronRight size={12} /> : null}
        </span>
        <span className={classes.explainKind}>{KIND_LABELS[node.kind]}</span>
        <span className={classes.explainLabel}>{node.label}</span>
        <span className={classes.explainExpr}>{node.expr}</span>
        <span className={classes.explainResult} data-tone={tone}>
          {text}
        </span>
      </button>
      {open && samples.length > 0 ? (
        <ul className={classes.explainSamples} style={{ paddingLeft: 40 + depth * 16 }}>
          {samples.map((sample, index) => (
            <li key={index}>{sampleText(sample)}</li>
          ))}
          {(node.result?.series ?? 0) > samples.length ? <li>…</li> : null}
        </ul>
      ) : null}
      {node.children.length > 0 ? (
        <ul className={classes.explainList}>
          {node.children.map((child, index) => (
            <Node key={index} node={child} depth={depth + 1} />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

/**
 * A query as a tree, every step evaluated on its own at the dashboard's end time: where a query
 * returns nothing, the tree shows which step lost the series.
 */
export function ExplainTree({
  query,
  end,
  range,
  variables,
}: {
  query: string;
  end: number;
  range: number;
  variables: Variables;
}) {
  const explain = useQuery({
    queryKey: ["metrics", "explain", query, end, range, variables],
    queryFn: () => metricsApi.explain({ query, time: end, range, variables }),
    enabled: query.trim() !== "",
    retry: false,
  });
  if (!query.trim()) return null;
  if (explain.isPending) return <p className={classes.explainNote}>Explaining…</p>;
  if (explain.error) {
    return (
      <p className={classes.explainNote} data-tone="error">
        {explain.error instanceof MetricsApiError ? explain.error.message : "Explain failed"}
      </p>
    );
  }
  return (
    <ul className={classes.explainTree} aria-label="Query explained">
      <Node node={explain.data} depth={0} />
    </ul>
  );
}
