import { Badge, Button, Drawer, Switch, Tabs, Text } from "@octofhir/ui-kit";
import { Database, RefreshCw, Zap } from "lucide-react";
import { useEffect, useState } from "react";
import { parseExplainTarget, useExplainQuery } from "../hooks/useExplainQuery";
import styles from "./ExplainPanel.module.css";

interface ExplainPanelProps {
  opened: boolean;
  onClose: () => void;
  /** The current console request path, e.g. "/fhir/Patient?name=x". */
  path: string;
}

interface PgPlanNode {
  "Node Type"?: string;
  "Relation Name"?: string;
  "Total Cost"?: number;
  "Plan Rows"?: number;
  Plans?: PgPlanNode[];
  [key: string]: unknown;
}

export function ExplainPanel({ opened, onClose, path }: ExplainPanelProps) {
  const explain = useExplainQuery();
  const [analyze, setAnalyze] = useState(false);
  const { resourceType, query } = parseExplainTarget(path);

  const run = () => {
    if (!resourceType) return;
    explain.mutate({ resourceType, query, analyze });
  };

  // Auto-run on the rising edge of `opened` only (not on every keystroke).
  // biome-ignore lint/correctness/useExhaustiveDependencies: intentional open-edge trigger
  useEffect(() => {
    if (opened && resourceType) {
      explain.mutate({ resourceType, query, analyze });
    }
  }, [opened]);

  const result = explain.data;

  return (
    <Drawer
      open={opened}
      onOpenChange={(next) => !next && onClose()}
      placement="right"
      size={640}
      title="Explain Query"
    >
      <div className={styles.root}>
        <div className={styles.toolbar}>
          <div className={styles.target}>
            <Badge size="sm" variant="light">
              {resourceType || "—"}
            </Badge>
            <Text size="sm" c="dimmed" truncate>
              {query ? `?${query}` : "(no search params)"}
            </Text>
          </div>
          <div className={styles.toolbarActions}>
            <Switch checked={analyze} onChange={setAnalyze} label="ANALYZE" />
            <Button
              size="xs"
              variant="light"
              onClick={run}
              loading={explain.isPending}
              disabled={!resourceType}
            >
              <Button.Icon>
                <RefreshCw size={14} />
              </Button.Icon>
              Run
            </Button>
          </div>
        </div>

        {!resourceType ? (
          <div className={styles.empty}>
            <Text c="dimmed">Enter a resource type to explain (e.g. /fhir/Patient?name=…).</Text>
          </div>
        ) : explain.isError ? (
          <div className={styles.empty}>
            <Text c="fire">{(explain.error as Error)?.message ?? "Explain failed"}</Text>
          </div>
        ) : !result ? (
          <div className={styles.empty}>
            <Text c="dimmed">{explain.isPending ? "Planning…" : "Run to see the plan."}</Text>
          </div>
        ) : (
          <Tabs defaultValue="ir" className={styles.tabs}>
            <Tabs.List>
              <Tabs.Tab value="ir">
                <Zap size={14} /> IR
              </Tabs.Tab>
              <Tabs.Tab value="sql">
                <Database size={14} /> SQL
              </Tabs.Tab>
              <Tabs.Tab value="plan">Plan</Tabs.Tab>
            </Tabs.List>

            <Tabs.Panel value="ir" className={styles.panel}>
              <IrView result={result} />
            </Tabs.Panel>
            <Tabs.Panel value="sql" className={styles.panel}>
              <SqlView result={result} />
            </Tabs.Panel>
            <Tabs.Panel value="plan" className={styles.panel}>
              <PlanView plan={result.explain_plan} analyzed={result.analyzed} />
            </Tabs.Panel>
          </Tabs>
        )}
      </div>
    </Drawer>
  );
}

function IrView({ result }: { result: ReturnType<typeof useExplainQuery>["data"] }) {
  const predicates = result?.parsed_ir?.predicates ?? [];
  if (predicates.length === 0) {
    return <Text c="dimmed">No predicates (matches all of this resource type).</Text>;
  }
  return (
    <div className={styles.irList}>
      {predicates.map((p) => (
        <div key={`${p.param_code}-${p.sql_shape}`} className={styles.irRow}>
          <div className={styles.irHead}>
            <Text size="sm" fw={600}>
              {p.param_code}
            </Text>
            <Badge size="sm" variant="light">
              {p.search_type}
            </Badge>
            <Badge size="sm" color={p.index_backed ? "primary" : "warm"}>
              {p.index_backed ? "index" : "scan"}
            </Badge>
          </div>
          <Text size="xs" c="dimmed" className={styles.mono}>
            {p.strategy}
            {p.expected_index ? ` · ${p.expected_index}` : ""}
          </Text>
          <pre className={styles.shape}>{p.sql_shape}</pre>
        </div>
      ))}
    </div>
  );
}

function SqlView({ result }: { result: ReturnType<typeof useExplainQuery>["data"] }) {
  if (!result) return null;
  return (
    <div className={styles.sqlView}>
      <pre className={styles.code}>{result.sql}</pre>
      {result.params.length > 0 && (
        <div className={styles.params}>
          {result.params.map((p, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: positional bind params
            <Badge key={i} size="sm" variant="light">
              ${i + 1}: {p}
            </Badge>
          ))}
        </div>
      )}
      {result.unknown_params.length > 0 && (
        <Text size="xs" c="warm">
          Unknown params: {result.unknown_params.map((u) => u.name).join(", ")}
        </Text>
      )}
    </div>
  );
}

function PlanView({ plan, analyzed }: { plan: unknown; analyzed: boolean }) {
  const root = Array.isArray(plan) ? (plan[0] as Record<string, unknown> | undefined) : undefined;
  const planNode = root?.Plan as PgPlanNode | undefined;
  const planningTime = root?.["Planning Time"] as number | undefined;
  const executionTime = root?.["Execution Time"] as number | undefined;

  const seqScans: string[] = [];
  const walk = (n?: PgPlanNode) => {
    if (!n) return;
    if (n["Node Type"] === "Seq Scan" && n["Relation Name"]) {
      seqScans.push(n["Relation Name"]);
    }
    n.Plans?.forEach(walk);
  };
  walk(planNode);

  return (
    <div className={styles.planView}>
      <div className={styles.planMeta}>
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
            index-only path
          </Badge>
        )}
      </div>
      <pre className={styles.code}>{JSON.stringify(planNode ?? plan, null, 2)}</pre>
    </div>
  );
}
