import { Badge, Button, Drawer, SegmentedControl, Switch, Tabs, Text } from "@octofhir/ui-kit";
import { Copy, Database, RefreshCw, Zap } from "lucide-react";
import { useEffect, useState } from "react";
import { parseExplainTarget, useExplainQuery } from "../hooks/useExplainQuery";
import styles from "./ExplainPanel.module.css";
import { PlanFlow } from "./PlanFlow";

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
      size={860}
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
              <Tabs.Tab value="graph">Graph</Tabs.Tab>
              <Tabs.Tab value="plan">Plan</Tabs.Tab>
            </Tabs.List>

            <Tabs.Panel value="ir" className={styles.panel}>
              <IrView result={result} />
            </Tabs.Panel>
            <Tabs.Panel value="sql" className={styles.panel}>
              <SqlView result={result} />
            </Tabs.Panel>
            <Tabs.Panel value="graph" className={styles.panel}>
              <PlanFlow plan={result.explain_plan} />
            </Tabs.Panel>
            <Tabs.Panel value="plan" className={styles.panel}>
              <PlanView result={result} />
            </Tabs.Panel>
          </Tabs>
        )}
      </div>
    </Drawer>
  );
}

function IrView({ result }: { result: ReturnType<typeof useExplainQuery>["data"] }) {
  const predicates = result?.parsed_ir?.predicates ?? [];
  const parsedParams = result?.parsed_params ?? [];
  return (
    <div className={styles.irList}>
      {parsedParams.length > 0 && (
        <div className={styles.irSection}>
          <Text className={styles.sectionLabel}>Parsed parameters</Text>
          {parsedParams.map((p) => (
            <div key={p.name} className={styles.paramRow}>
              <Text size="xs" fw={600} className={styles.mono}>
                {p.name}
              </Text>
              <Text size="xs" c="dimmed" className={styles.mono}>
                {p.values.join(", ")}
              </Text>
            </div>
          ))}
        </div>
      )}

      <div className={styles.irSection}>
        <Text className={styles.sectionLabel}>Predicates</Text>
        {predicates.length === 0 ? (
          <Text size="sm" c="dimmed">
            No indexable predicates modelled (chains / _has run as SQL subqueries — see SQL tab).
          </Text>
        ) : (
          predicates.map((p) => (
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
          ))
        )}
      </div>
    </div>
  );
}

function SqlView({ result }: { result: ReturnType<typeof useExplainQuery>["data"] }) {
  if (!result) return null;
  return (
    <div className={styles.sqlView}>
      <div className={styles.sqlHead}>
        <Text className={styles.sectionLabel}>Runnable SQL</Text>
        <Button
          size="xs"
          variant="subtle"
          onClick={() => navigator.clipboard?.writeText(result.runnable_sql)}
        >
          <Button.Icon>
            <Copy size={13} />
          </Button.Icon>
          Copy
        </Button>
      </div>
      <pre className={styles.code}>{result.runnable_sql}</pre>

      {result.params.length > 0 && (
        <>
          <Text className={styles.sectionLabel}>Bindings</Text>
          <div className={styles.bindList}>
            {result.params.map((p) => (
              <div key={p.placeholder} className={styles.bindRow}>
                <Badge size="sm" variant="light">
                  {p.placeholder}
                </Badge>
                <Text size="xs" c="dimmed">
                  {p.kind}
                </Text>
                <Text size="xs" className={styles.mono}>
                  {p.value}
                </Text>
              </div>
            ))}
          </div>
        </>
      )}

      {result.unknown_params.length > 0 && (
        <Text size="xs" c="warm">
          Unknown params: {result.unknown_params.map((u) => u.name).join(", ")}
        </Text>
      )}
    </div>
  );
}

function PlanView({ result }: { result: ReturnType<typeof useExplainQuery>["data"] }) {
  const [format, setFormat] = useState<"text" | "json">("text");
  if (!result) return null;
  const plan = result.explain_plan;
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
        {result.analyzed && executionTime != null && (
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
        <div className={styles.planFormat}>
          <SegmentedControl
            size="xs"
            options={[
              { label: "Text", value: "text" },
              { label: "JSON", value: "json" },
            ]}
            value={format}
            onChange={(v) => setFormat(v === "json" ? "json" : "text")}
          />
        </div>
      </div>
      <pre className={styles.code}>
        {format === "text"
          ? result.explain_text || "(no text plan)"
          : JSON.stringify(planNode ?? plan, null, 2)}
      </pre>
    </div>
  );
}
