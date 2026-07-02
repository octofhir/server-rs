// Dataflow graph — renders the notebook cell dependency DAG (who feeds whom via
// ${refs} + chart/pipeline inputs + config.contextCell). Nodes colored by run
// status. See docs/ui-notebooks-plan.md §12c.

import { Chart, type EChartsCoreOption } from "@octofhir/ui-kit";
import { useMemo } from "react";
import { depMap } from "../model/dag";
import type { CellStatus, Notebook } from "../model/notebook";
import classes from "../NotebookEditor.module.css";

const STATUS_COLOR: Record<CellStatus, string> = {
  idle: "#94a3b8",
  running: "#f59e0b",
  stale: "#f59e0b",
  ok: "#10b981",
  error: "#ef4444",
};

const TYPE_ABBR: Record<string, string> = {
  markdown: "MD",
  fhirpath: "FP",
  "sql-on-fhir": "SoF",
  sql: "SQL",
  cql: "CQL",
  graphql: "GQL",
  rest: "REST",
  pipeline: "PIPE",
  chart: "CHART",
  input: "INPUT",
};

interface Props {
  notebook: Notebook;
  statuses: Record<string, CellStatus>;
}

export function DataflowGraph({ notebook, statuses }: Props) {
  const option = useMemo<EChartsCoreOption>(() => {
    const deps = depMap(notebook);
    const labelById = new Map<string, string>();
    const nodes = notebook.cells.map((c, i) => {
      const label = c.name ? `${c.name}` : `${TYPE_ABBR[c.type] ?? c.type} #${i + 1}`;
      labelById.set(c.id, label);
      const status = statuses[c.id] ?? "idle";
      return {
        name: c.id,
        symbolSize: c.name ? 42 : 30,
        itemStyle: { color: STATUS_COLOR[status] },
        label: { show: true },
        category: c.type,
      };
    });
    const links: { source: string; target: string }[] = [];
    for (const [id, set] of deps) {
      for (const dep of set) links.push({ source: dep, target: id });
    }

    return {
      tooltip: {
        formatter: (p: { dataType?: string; name?: string }) =>
          p.dataType === "node" ? (labelById.get(p.name ?? "") ?? p.name ?? "") : "",
      },
      series: [
        {
          type: "graph",
          layout: "force",
          roam: true,
          draggable: true,
          edgeSymbol: ["none", "arrow"],
          edgeSymbolSize: 9,
          force: { repulsion: 260, edgeLength: 130, gravity: 0.1 },
          label: {
            show: true,
            position: "bottom",
            formatter: (p: { name?: string }) => labelById.get(p.name ?? "") ?? "",
            fontSize: 11,
          },
          lineStyle: { color: "#94a3b8", width: 1.5, curveness: 0.05, opacity: 0.7 },
          data: nodes,
          links,
        },
      ],
    };
  }, [notebook, statuses]);

  const hasEdges = useMemo(() => {
    for (const set of depMap(notebook).values()) if (set.size) return true;
    return false;
  }, [notebook]);

  return (
    <div className={classes.graphTab}>
      {notebook.cells.length === 0 ? (
        <div className={classes.chartEmpty}>No cells yet.</div>
      ) : (
        <>
          {!hasEdges && (
            <div className={classes.graphHint}>
              No data dependencies yet. Name a cell, then reference it via{" "}
              <code>
                {"$"}
                {"{name}"}
              </code>
              , a chart/pipeline input, or <code>contextCell</code>.
            </div>
          )}
          <Chart option={option} height={520} notMerge />
        </>
      )}
    </div>
  );
}
