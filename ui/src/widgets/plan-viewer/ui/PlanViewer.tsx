import { Badge, SegmentedControl, Text } from "@octofhir/ui-kit";
import { useMemo, useState } from "react";
import { PlanFlow } from "./PlanFlow";
import styles from "./PlanViewer.module.css";
import { normalizePlan, planToText } from "./planText";

type PlanView = "graph" | "text" | "json";

interface PlanViewerProps {
  /** Raw EXPLAIN (FORMAT JSON) output (array, object, or JSON string). */
  plan: unknown;
  /** Optional native EXPLAIN text; falls back to a tree rendered from the JSON plan. */
  text?: string;
  /** Whether the plan was produced with ANALYZE (shows actual timing). */
  analyzed?: boolean;
}

interface PgPlanNode {
  "Node Type"?: string;
  "Relation Name"?: string;
  Plans?: PgPlanNode[];
  [key: string]: unknown;
}

export function PlanViewer({ plan, text, analyzed }: PlanViewerProps) {
  const [view, setView] = useState<PlanView>("graph");

  const roots = useMemo(() => normalizePlan(plan), [plan]);
  const textPlan = useMemo(() => text || planToText(plan), [text, plan]);
  const jsonPlan = useMemo(() => (roots ? JSON.stringify(roots, null, 2) : ""), [roots]);

  const { planningTime, executionTime, seqScans } = useMemo(() => {
    const root = roots?.[0];
    const scans: string[] = [];
    const walk = (n?: PgPlanNode) => {
      if (!n) return;
      if (n["Node Type"] === "Seq Scan" && n["Relation Name"]) scans.push(n["Relation Name"]);
      n.Plans?.forEach(walk);
    };
    walk(root?.Plan as PgPlanNode | undefined);
    return {
      planningTime: root?.["Planning Time"] as number | undefined,
      executionTime: root?.["Execution Time"] as number | undefined,
      seqScans: scans,
    };
  }, [roots]);

  if (!roots) {
    return (
      <Text c="dimmed" ta="center" py="xl" size="sm">
        No execution plan available
      </Text>
    );
  }

  return (
    <div className={styles.root}>
      <div className={styles.toolbar}>
        <SegmentedControl
          size="xs"
          value={view}
          onChange={(v) => setView(v as PlanView)}
          options={[
            { label: "Graph", value: "graph" },
            { label: "Text", value: "text" },
            { label: "JSON", value: "json" },
          ]}
        />
        <div className={styles.meta}>
          {planningTime != null && (
            <Badge size="sm" variant="light">
              plan {planningTime.toFixed(2)}ms
            </Badge>
          )}
          {analyzed && executionTime != null && (
            <Badge size="sm" color="primary">
              exec {executionTime.toFixed(2)}ms
            </Badge>
          )}
          {seqScans.length > 0 ? (
            <Badge size="sm" color="fire">
              seq scan: {seqScans.join(", ")}
            </Badge>
          ) : (
            <Badge size="sm" color="primary">
              index path
            </Badge>
          )}
        </div>
      </div>

      <div className={styles.body}>
        {view === "graph" && <PlanFlow plan={roots} />}
        {view === "text" && <pre className={styles.code}>{textPlan || "(no text plan)"}</pre>}
        {view === "json" && <pre className={styles.code}>{jsonPlan}</pre>}
      </div>
    </div>
  );
}
