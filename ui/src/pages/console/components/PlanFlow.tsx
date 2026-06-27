import {
  Background,
  Controls,
  type Edge,
  Handle,
  MarkerType,
  MiniMap,
  type Node,
  type NodeProps,
  Position,
  ReactFlow,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useMemo } from "react";
import styles from "./PlanFlow.module.css";

interface PgPlan {
  "Node Type"?: string;
  "Relation Name"?: string;
  "Index Name"?: string;
  Alias?: string;
  "Join Type"?: string;
  "Total Cost"?: number;
  "Startup Cost"?: number;
  "Plan Rows"?: number;
  "Actual Rows"?: number;
  "Actual Loops"?: number;
  "Rows Removed by Filter"?: number;
  Plans?: PgPlan[];
  [key: string]: unknown;
}

type Cond = { label: string; value: string };

type PlanNodeData = {
  nodeType: string;
  detail?: string;
  conds: Cond[];
  joinType?: string;
  cost?: number;
  planRows?: number;
  actualRows?: number;
  loops?: number;
  rowsRemoved?: number;
  kind: "scan" | "index" | "join" | "sort" | "agg" | "other";
  warn: boolean;
};

const NODE_W = 260;
const X_GAP = 48;
const Y_GAP = 200;

// Condition fields worth surfacing, in display order.
const COND_KEYS: [string, string][] = [
  ["Index Cond", "index"],
  ["Recheck Cond", "recheck"],
  ["Hash Cond", "hash"],
  ["Merge Cond", "merge"],
  ["Join Filter", "join"],
  ["Filter", "filter"],
  ["One-Time Filter", "once"],
];

function classify(nodeType: string): PlanNodeData["kind"] {
  const t = nodeType.toLowerCase();
  if (t.includes("seq scan")) return "scan";
  if (t.includes("index") || t.includes("bitmap")) return "index";
  if (t.includes("loop") || t.includes("join")) return "join";
  if (t.includes("sort") || t.includes("merge append")) return "sort";
  if (t.includes("aggregate") || t.includes("group") || t.includes("unique")) return "agg";
  return "other";
}

function PlanNodeCard({ data }: NodeProps) {
  const d = data as PlanNodeData;
  return (
    <div className={styles.node} data-kind={d.kind} data-warn={d.warn ? "1" : undefined}>
      <Handle type="target" position={Position.Top} className={styles.handle} />
      <div className={styles.nodeHead}>
        <span className={styles.nodeType}>{d.nodeType}</span>
        {d.joinType ? <span className={styles.tag}>{d.joinType}</span> : null}
        {d.loops && d.loops > 1 ? <span className={styles.loops}>×{d.loops}</span> : null}
      </div>
      {d.detail ? <div className={styles.nodeDetail}>{d.detail}</div> : null}
      {d.conds.map((c) => (
        <div key={c.label} className={styles.cond} title={`${c.label}: ${c.value}`}>
          <span className={styles.condLabel}>{c.label}</span>
          <span className={styles.condValue}>{c.value}</span>
        </div>
      ))}
      <div className={styles.nodeMeta}>
        {d.cost != null ? <span>cost {d.cost.toFixed(1)}</span> : null}
        {d.actualRows != null ? (
          <span className={d.warn ? styles.metaWarn : undefined}>
            {d.actualRows} / {d.planRows ?? "?"} rows
          </span>
        ) : d.planRows != null ? (
          <span>~{d.planRows} rows</span>
        ) : null}
        {d.rowsRemoved && d.rowsRemoved > 0 ? (
          <span className={styles.metaWarn}>−{d.rowsRemoved} filtered</span>
        ) : null}
      </div>
      <Handle type="source" position={Position.Bottom} className={styles.handle} />
    </div>
  );
}

const nodeTypes = { plan: PlanNodeCard };

function truncate(s: string, n = 64): string {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s;
}

function extractConds(plan: PgPlan): Cond[] {
  const out: Cond[] = [];
  for (const [key, label] of COND_KEYS) {
    const v = plan[key];
    if (typeof v === "string" && v.trim()) out.push({ label, value: truncate(v) });
  }
  return out;
}

function buildGraph(root: PgPlan): { nodes: Node[]; edges: Edge[] } {
  const nodes: Node[] = [];
  const edges: Edge[] = [];
  let leafX = 0;
  let id = 0;

  const layout = (plan: PgPlan, depth: number, parentId: string | null): number => {
    const myId = `n${id++}`;
    const children = plan.Plans ?? [];
    let x: number;
    if (children.length === 0) {
      x = leafX * (NODE_W + X_GAP);
      leafX++;
    } else {
      const xs = children.map((c) => layout(c, depth + 1, myId));
      x = (Math.min(...xs) + Math.max(...xs)) / 2;
    }

    const nodeType = plan["Node Type"] ?? "Node";
    const planRows = plan["Plan Rows"];
    const actualRows = plan["Actual Rows"];
    const loops = plan["Actual Loops"];
    const warn =
      actualRows != null &&
      planRows != null &&
      planRows > 0 &&
      (actualRows / planRows > 10 || planRows / actualRows > 10);

    nodes.push({
      id: myId,
      type: "plan",
      position: { x, y: depth * Y_GAP },
      data: {
        nodeType,
        detail: plan["Index Name"]
          ? `using ${plan["Index Name"]}${plan["Relation Name"] ? ` on ${plan["Relation Name"]}` : ""}`
          : plan["Relation Name"]
            ? `on ${plan["Relation Name"]}${plan.Alias && plan.Alias !== plan["Relation Name"] ? ` ${plan.Alias}` : ""}`
            : undefined,
        conds: extractConds(plan),
        joinType: plan["Join Type"],
        cost: plan["Total Cost"],
        planRows,
        actualRows,
        loops,
        rowsRemoved: plan["Rows Removed by Filter"],
        kind: classify(nodeType),
        warn,
      } satisfies PlanNodeData,
    });

    if (parentId) {
      const rowLabel =
        actualRows != null ? `${actualRows} rows` : planRows != null ? `~${planRows} rows` : "";
      edges.push({
        id: `${parentId}-${myId}`,
        source: parentId,
        target: myId,
        type: "smoothstep",
        label: rowLabel,
        labelShowBg: true,
        labelBgPadding: [6, 2],
        labelBgBorderRadius: 6,
        labelBgStyle: { fill: "var(--octo-surface-3)" },
        labelStyle: { fill: "var(--octo-text-secondary)", fontSize: 11, fontWeight: 600 },
        animated: (loops ?? 1) > 1,
        style: { stroke: "var(--octo-accent-primary)", strokeWidth: 2 },
        markerEnd: {
          type: MarkerType.ArrowClosed,
          width: 18,
          height: 18,
          color: "var(--octo-accent-primary)",
        },
      });
    }
    return x;
  };

  layout(root, 0, null);
  return { nodes, edges };
}

export function PlanFlow({ plan }: { plan: unknown }) {
  const { nodes, edges } = useMemo(() => {
    const root = Array.isArray(plan)
      ? ((plan[0] as Record<string, unknown> | undefined)?.Plan as PgPlan | undefined)
      : undefined;
    if (!root) return { nodes: [], edges: [] };
    return buildGraph(root);
  }, [plan]);

  if (nodes.length === 0) {
    return <div className={styles.empty}>No plan to graph.</div>;
  }

  return (
    <div className={styles.flowWrap}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        fitView
        minZoom={0.2}
        proOptions={{ hideAttribution: true }}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
      >
        <Background gap={16} />
        <MiniMap pannable zoomable className={styles.minimap} />
        <Controls showInteractive={false} />
      </ReactFlow>
    </div>
  );
}
