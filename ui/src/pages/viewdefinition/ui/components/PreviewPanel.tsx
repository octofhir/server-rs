import {
  Badge,
  ChartBuilder,
  type ChartSpec,
  type EChartsType,
  Spin,
  suggestChartSpec,
  type TabularData,
  Text,
} from "@octofhir/ui-kit";
import { ChartColumnBig, Table as TableIcon, Waypoints } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { RunResult, SqlResult } from "../../lib/useViewDefinition";
import classes from "./PreviewPanel.module.css";
import { ResultTable } from "./ResultTable";
import { SQLPreview } from "./SQLPreview";

type PreviewTab = "table" | "chart" | "plan";

interface PreviewPanelProps {
  runResult: RunResult | null;
  sqlResult: SqlResult | null;
  onRefreshSql: () => void;
  isLoading: boolean;
}

export function PreviewPanel({ runResult, sqlResult, onRefreshSql, isLoading }: PreviewPanelProps) {
  const [tab, setTab] = useState<PreviewTab>("table");
  const hasRows = !!runResult && runResult.rowCount > 0;

  const chartRef = useRef<EChartsType | null>(null);
  // Convert the FHIR-shaped result (rows as objects keyed by column name) into
  // the matrix TabularData the chart builder expects.
  const tabular = useMemo<TabularData>(() => {
    const columns = (runResult?.columns ?? []).map((c) => c.name);
    const rows = (runResult?.rows ?? []).map((row) => {
      const obj = (row ?? {}) as Record<string, unknown>;
      return columns.map((name) => obj[name] ?? null);
    });
    return { columns, rows };
  }, [runResult]);
  const [chartSpec, setChartSpec] = useState<ChartSpec | null>(null);

  // Seed a chart spec the first time the Chart tab opens with data.
  useEffect(() => {
    if (tab !== "chart" || !hasRows) return;
    setChartSpec((prev) => prev ?? suggestChartSpec(tabular));
  }, [tab, hasRows, tabular]);

  // Keep the latest onRefreshSql in a ref so switching to the Plan tab fires it
  // exactly once, without re-running an effect every time the callback identity
  // changes (which would otherwise loop forever).
  const refreshSqlRef = useRef(onRefreshSql);
  refreshSqlRef.current = onRefreshSql;

  const selectTab = (id: PreviewTab) => {
    setTab(id);
    if (id === "plan") refreshSqlRef.current();
  };

  const tabBtn = (id: PreviewTab, icon: React.ReactNode, text: string, show = true) =>
    show ? (
      <button
        type="button"
        className={tab === id ? classes.tabActive : classes.tab}
        onClick={() => selectTab(id)}
      >
        {icon}
        {text}
      </button>
    ) : null;

  return (
    <div className={classes.panel}>
      <div className={classes.bar}>
        <div className={classes.tabs}>
          {tabBtn("table", <TableIcon size={13} />, "Table")}
          {tabBtn("chart", <ChartColumnBig size={13} />, "Chart")}
          {tabBtn("plan", <Waypoints size={13} />, "Plan")}
        </div>
        <div className={classes.meta}>
          {isLoading && <Spin size="sm" />}
          {runResult && (
            <Text size="xs" c="dimmed">
              {runResult.rowCount} rows · {runResult.columns.length} cols
            </Text>
          )}
        </div>
      </div>

      <div className={classes.body}>
        {tab === "table" &&
          (runResult ? (
            <div className={classes.scroll}>
              <ResultTable result={runResult} />
            </div>
          ) : (
            <Empty label="Run the view to preview rows." />
          ))}

        {tab === "chart" &&
          (hasRows && chartSpec ? (
            <div className={classes.chart}>
              <ChartBuilder
                data={tabular}
                spec={chartSpec}
                onSpecChange={setChartSpec}
                chartRef={chartRef}
              />
            </div>
          ) : (
            <Empty label="Run the view to chart its output." />
          ))}

        {tab === "plan" && (
          <div className={classes.scroll}>
            {sqlResult ? (
              <>
                <Badge size="xs" variant="light" className={classes.planBadge}>
                  Generated PostgreSQL · informational
                </Badge>
                <SQLPreview result={sqlResult} isLoading={isLoading} error={null} />
              </>
            ) : (
              <Empty label="The compiled SQL plan appears here." />
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function Empty({ label }: { label: string }) {
  return (
    <div className={classes.empty}>
      <TableIcon size={30} strokeWidth={1.2} />
      <Text size="sm" c="dimmed">
        {label}
      </Text>
    </div>
  );
}
