import {
  ActionIcon,
  Badge,
  ChartBuilder,
  type ChartSpec,
  type EChartsType,
  suggestChartSpec,
  type TabularData,
  Text,
  TextInput,
  Tooltip,
} from "@octofhir/ui-kit";
import {
  ArrowDownToLine,
  Braces,
  ChartColumnBig,
  FileSpreadsheet,
  ImageDown,
  Search,
  Table as TableIcon,
  Waypoints,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import classes from "../DbConsolePage.module.css";
import type { StreamEntry } from "../types";
import { ExplainPane } from "./ExplainPane";
import { RawResultView } from "./RawResultView";
import { ResultsTable } from "./ResultsTable";
import {
  downloadChartPNG,
  downloadCSV,
  hashQuery,
  loadChartSpec,
  resultToCSV,
  resultToJSON,
  saveChartSpec,
} from "./resultExport";

interface ResultPanelProps {
  entry: StreamEntry | undefined;
}

type ResultTab = "table" | "raw" | "explain" | "chart";

function copyText(text: string): void {
  navigator.clipboard?.writeText(text).catch(() => {});
}

export function ResultPanel({ entry }: ResultPanelProps) {
  const [tab, setTab] = useState<ResultTab>("table");
  const [search, setSearch] = useState("");

  const result = entry?.status === "success" ? entry.result : undefined;
  const error = entry?.status === "error" && entry.error ? new Error(entry.error) : null;
  const isPending = entry?.status === "pending";
  const hasRows = !!result && result.rowCount > 0;
  const hasExplain = !!entry?.explainData;

  const jsonText = useMemo(() => (result ? resultToJSON(result) : ""), [result]);
  const csvText = useMemo(() => (result ? resultToCSV(result.columns, result.rows) : ""), [result]);

  const dataTab = tab === "table" || tab === "raw";

  // --- Chart tab state ---
  const chartRef = useRef<EChartsType | null>(null);
  const tabular = useMemo<TabularData>(
    () => ({ columns: result?.columns ?? [], rows: result?.rows ?? [] }),
    [result]
  );
  const queryHash = useMemo(() => (entry ? hashQuery(entry.query) : ""), [entry]);
  const [chartSpec, setChartSpec] = useState<ChartSpec | null>(null);

  // Load (or seed) the per-query chart spec the first time the Chart tab opens.
  useEffect(() => {
    if (tab !== "chart" || !hasRows) return;
    setChartSpec(loadChartSpec(queryHash) ?? suggestChartSpec(tabular));
  }, [tab, queryHash, hasRows, tabular]);

  const handleSpecChange = (spec: ChartSpec) => {
    setChartSpec(spec);
    saveChartSpec(queryHash, spec);
  };

  if (!entry) {
    return (
      <div className={classes.resultEmpty}>
        <TableIcon size={34} strokeWidth={1.2} />
        <Text size="sm" fw={600}>
          No results yet
        </Text>
        <Text size="xs" c="dimmed">
          Execute a query to inspect rows, raw JSON, and the query plan.
        </Text>
      </div>
    );
  }

  return (
    <div className={classes.resultPanel}>
      <div className={classes.resultBar}>
        <div className={classes.resultTabs}>
          <button
            type="button"
            className={tab === "table" ? classes.resultTabActive : classes.resultTab}
            onClick={() => setTab("table")}
          >
            <TableIcon size={13} />
            Table
          </button>
          <button
            type="button"
            className={tab === "raw" ? classes.resultTabActive : classes.resultTab}
            onClick={() => setTab("raw")}
          >
            <Braces size={13} />
            Raw
          </button>
          {hasExplain && (
            <button
              type="button"
              className={tab === "explain" ? classes.resultTabActive : classes.resultTab}
              onClick={() => setTab("explain")}
            >
              <Waypoints size={13} />
              Explain
            </button>
          )}
          {hasRows && (
            <button
              type="button"
              className={tab === "chart" ? classes.resultTabActive : classes.resultTab}
              onClick={() => setTab("chart")}
            >
              <ChartColumnBig size={13} />
              Chart
            </button>
          )}
        </div>

        <div className={classes.resultMeta}>
          {dataTab && hasRows && (
            <TextInput
              size="xs"
              value={search}
              onChange={(value) => setSearch(value)}
              placeholder="Filter rows…"
              leftSection={<Search size={13} />}
              className={classes.resultSearch}
              aria-label="Filter result rows"
            />
          )}
          {isPending && (
            <Badge size="xs" variant="light" color="warm">
              running…
            </Badge>
          )}
          {entry.status === "error" && (
            <Badge size="xs" variant="light" color="fire">
              error
            </Badge>
          )}
          {result && (
            <Text size="xs" c="dimmed" className={classes.nowrap}>
              {result.rowCount} rows
            </Text>
          )}
          {entry.executionTimeMs != null && (
            <Text size="xs" c="dimmed" ff="monospace" className={classes.nowrap}>
              {entry.executionTimeMs}ms
            </Text>
          )}
          {tab === "chart" && hasRows && (
            <Tooltip label="Export PNG">
              <ActionIcon
                variant="subtle"
                size="sm"
                onClick={() => chartRef.current && downloadChartPNG(chartRef.current)}
              >
                <ImageDown size={14} />
              </ActionIcon>
            </Tooltip>
          )}
          {hasRows && (
            <>
              <Tooltip label="Copy as JSON">
                <ActionIcon variant="subtle" size="sm" onClick={() => copyText(jsonText)}>
                  <Braces size={14} />
                </ActionIcon>
              </Tooltip>
              <Tooltip label="Copy as CSV">
                <ActionIcon variant="subtle" size="sm" onClick={() => copyText(csvText)}>
                  <FileSpreadsheet size={14} />
                </ActionIcon>
              </Tooltip>
              <Tooltip label="Download CSV">
                <ActionIcon
                  variant="subtle"
                  size="sm"
                  onClick={() => result && downloadCSV(result.columns, result.rows)}
                >
                  <ArrowDownToLine size={14} />
                </ActionIcon>
              </Tooltip>
            </>
          )}
        </div>
      </div>

      <div className={classes.resultBody}>
        {tab === "table" && (
          <ResultsTable data={result} error={error} isPending={isPending} filter={search} />
        )}
        {tab === "raw" && (
          <RawResultView data={result} error={error} isPending={isPending} filter={search} />
        )}
        {tab === "explain" && (
          <ExplainPane data={entry.explainData} error={null} isPending={false} />
        )}
        {tab === "chart" && hasRows && chartSpec && (
          <div className={classes.resultChart}>
            <ChartBuilder
              data={tabular}
              spec={chartSpec}
              onSpecChange={handleSpecChange}
              chartRef={chartRef}
            />
          </div>
        )}
      </div>
    </div>
  );
}
