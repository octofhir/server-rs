import {
  Button,
  Chart,
  DataTable,
  type DataTableColumn,
  type DataTableSort,
  EmptyState,
  SegmentedRadioGroup,
  Sparkline,
  useColorScheme,
} from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { Gauge, Pause, Play, Send, X } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { usePaletteContribution } from "@/shared/command-palette";
import { useUrlFilters } from "@/shared/lib/url-filters";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import {
  $snapshots,
  intervalSeries,
  POLL_INTERVAL_MS,
  type RouteStats,
  routeQuantileSeries,
  routeStats,
  useMetricsFeed,
} from "@/entities/server-metrics";
import { meanLatencyOption, quantileOption, throughputOption } from "./charts";
import { formatMs, formatRate, formatShare } from "./format";
import classes from "./PerformancePage.module.css";

const WINDOWS = { "1m": 60_000, "5m": 300_000, "15m": 900_000 } as const;
type WindowKey = keyof typeof WINDOWS;

const URL_KEYS = { text: ["window"] } as const;

const CHART_HEIGHT = 200;

function isWindowKey(value: string | undefined): value is WindowKey {
  return value != null && value in WINDOWS;
}

function Stat({
  label,
  value,
  caption,
  trend,
}: {
  label: string;
  value: string;
  caption: string;
  trend?: (number | null)[];
}) {
  return (
    <div className={classes.cell}>
      <span className={classes.label}>{label}</span>
      <span className={classes.value}>{value}</span>
      <span className={classes.caption}>{caption}</span>
      {trend ? <Sparkline className={classes.spark} values={trend} /> : null}
    </div>
  );
}

function Num({ value, format }: { value: number | null; format: (value: number) => string }) {
  return value == null ? <span className={classes.muted}>—</span> : <span className={classes.num}>{format(value)}</span>;
}

const COLUMNS: DataTableColumn<RouteStats>[] = [
  {
    id: "route",
    header: "Route",
    accessor: (route) => route.path,
    sortable: true,
    cell: (route) => (
      <span className={classes.route}>
        <span className={classes.method}>{route.method}</span>
        <span className={classes.path}>{route.path}</span>
      </span>
    ),
  },
  {
    id: "rate",
    header: "Req/s",
    width: 84,
    align: "right",
    sortable: true,
    accessor: (route) => route.rate ?? route.requests / 1e9,
    cell: (route) => <Num value={route.rate} format={formatRate} />,
  },
  {
    id: "requests",
    header: "Requests",
    width: 92,
    align: "right",
    sortable: true,
    accessor: (route) => route.requests,
    cell: (route) => <Num value={route.requests} format={(value) => value.toLocaleString()} />,
  },
  {
    id: "mean",
    header: "Mean",
    width: 88,
    align: "right",
    sortable: true,
    accessor: (route) => route.meanMs ?? -1,
    cell: (route) => <Num value={route.meanMs} format={formatMs} />,
  },
  ...(["p50", "p95", "p99"] as const).map(
    (key): DataTableColumn<RouteStats> => ({
      id: key,
      header: <span title="Over the selected window">{key}</span>,
      width: 84,
      align: "right",
      sortable: true,
      accessor: (route) => route[key] ?? -1,
      cell: (route) => <Num value={route[key]} format={formatMs} />,
    })
  ),
  {
    id: "errors",
    header: "5xx",
    width: 72,
    align: "right",
    sortable: true,
    accessor: (route) => route.serverErrors,
    cell: (route) =>
      route.serverErrors > 0 ? (
        <span className={classes.errors}>
          <span className={classes.errorDot} aria-hidden="true" />
          {formatShare(route.serverErrors / Math.max(1, route.requests))}
        </span>
      ) : (
        <span className={classes.muted}>—</span>
      ),
  },
];

export function PerformancePage() {
  const navigate = useNavigate();
  const { colorScheme } = useColorScheme();
  const [paused, setPaused] = useState(false);
  const [filters, setFilters] = useUrlFilters<{ window?: string }>(URL_KEYS);
  const windowKey: WindowKey = isWindowKey(filters.window) ? filters.window : "5m";
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [sorting, setSorting] = useState<DataTableSort[]>([{ id: "rate", desc: true }]);
  const [showIdle, setShowIdle] = useState(false);

  const feed = useMetricsFeed(paused);
  const snapshots = useUnit($snapshots);
  const latest = snapshots.at(-1);
  const range = useMemo(() => {
    const to = latest?.at ?? Date.now();
    return { from: to - WINDOWS[windowKey], to };
  }, [latest, windowKey]);

  const inWindow = useMemo(
    // One scrape before the window gives its first interval a baseline.
    () => snapshots.filter((snapshot, index) => snapshot.at >= range.from || snapshots[index + 1]?.at >= range.from),
    [snapshots, range.from]
  );
  const intervals = useMemo(() => intervalSeries(inWindow), [inWindow]);
  // Servers that export a latency histogram give exact quantiles for any set of routes.
  const hasHistogram = (latest?.bounds.length ?? 0) > 0;
  const routes = useMemo(() => routeStats(inWindow, range.from), [inWindow, range.from]);
  const selected = routes.find((route) => route.key === selectedKey) ?? null;
  const busy = useMemo(() => routes.filter((route) => route.requests > 0), [routes]);
  const idleCount = routes.length - busy.length;

  const throughput = useMemo(() => throughputOption(intervals, range, colorScheme), [intervals, range, colorScheme]);
  const latency = useMemo(
    () =>
      selected
        ? quantileOption(routeQuantileSeries(inWindow, selected.key), range, colorScheme)
        : hasHistogram
          ? quantileOption(
              intervals.map((point) => ({ at: point.at, ...point.quantiles })),
              range,
              colorScheme
            )
          : meanLatencyOption(intervals, range, colorScheme),
    [selected, inWindow, intervals, range, colorScheme, hasHistogram]
  );

  const lastInterval = intervals.at(-1);
  const currentRate = lastInterval?.rate
    ? lastInterval.rate.ok + lastInterval.rate.client + lastInterval.rate.server
    : null;
  const totals = routes.reduce(
    (sum, route) => ({
      requests: sum.requests + route.requests,
      server: sum.server + route.serverErrors,
      client: sum.client + route.clientErrors,
    }),
    { requests: 0, server: 0, client: 0 }
  );
  const windowRate = intervals.length > 0 && inWindow.length > 1 ? totals.requests / ((range.to - inWindow[0].at) / 1000) : null;

  const togglePaused = useCallback(() => setPaused((value) => !value), []);
  const setWindow = useCallback(
    (value: string) => setFilters({ window: value === "5m" ? undefined : value }),
    [setFilters]
  );

  usePaletteContribution(
    "performance",
    useMemo(
      () => [
        {
          id: "performance-pause",
          label: paused ? "Resume live metrics" : "Pause live metrics",
          group: "Performance",
          icon: paused ? Play : Pause,
          onSelect: togglePaused,
        },
        ...(Object.keys(WINDOWS) as WindowKey[]).map((key) => ({
          id: `performance-window-${key}`,
          label: `Show the last ${key.replace("m", " min")}`,
          group: "Performance",
          icon: Gauge,
          onSelect: () => setWindow(key),
        })),
      ],
      [paused, togglePaused, setWindow]
    )
  );


  const unavailable = feed.error != null && snapshots.length === 0;
  const windowLabel = windowKey.replace("m", " min");

  return (
    <WorkspacePageLayout
      title="Performance"
      description="Live throughput and latency, read from the server's /metrics."
      className="page-enter"
      actions={
        <>
          <span className={classes.live} data-state={paused ? "paused" : feed.error ? "error" : "live"}>
            <span className={classes.liveDot} aria-hidden="true" />
            {paused ? "Paused" : feed.error ? "Scrape failed" : `Live · every ${POLL_INTERVAL_MS / 1000} s`}
          </span>
          <SegmentedRadioGroup
            aria-label="Time window"
            value={windowKey}
            onChange={setWindow}
            options={(Object.keys(WINDOWS) as WindowKey[]).map((key) => ({ value: key, label: key }))}
          />
          <Button variant="outline" onClick={togglePaused}>
            <Button.Icon side="left">{paused ? <Play size={14} /> : <Pause size={14} />}</Button.Icon>
            {paused ? "Resume" : "Pause"}
          </Button>
        </>
      }
    >
      {unavailable ? (
        <EmptyState
          size="promo"
          image={<Gauge width={32} height={32} aria-hidden="true" />}
          title="Metrics are unavailable"
          description={`The server did not answer /metrics: ${feed.error instanceof Error ? feed.error.message : "unknown error"}.`}
          actions={[{ text: "Try again", onClick: feed.retry }]}
        />
      ) : (
        <>
          <section className={classes.stats} aria-label="Current load">
            <Stat
              label="Throughput"
              value={currentRate == null ? "—" : `${formatRate(currentRate)} req/s`}
              caption={windowRate == null ? "Collecting…" : `${formatRate(windowRate)} req/s over ${windowLabel}`}
              trend={intervals.map((point) => (point.rate ? point.rate.ok + point.rate.client + point.rate.server : null))}
            />
            <Stat
              label="Mean latency"
              value={lastInterval?.meanMs == null ? "—" : formatMs(lastInterval.meanMs)}
              caption="All routes, last interval"
              trend={intervals.map((point) => point.meanMs)}
            />
            <Stat
              label="Server errors"
              value={totals.requests > 0 ? formatShare(totals.server / totals.requests) : "—"}
              caption={`${totals.server.toLocaleString()} 5xx · ${totals.client.toLocaleString()} 4xx over ${windowLabel}`}
            />
            <Stat
              label="In flight"
              value={latest ? String(latest.inFlight) : "—"}
              caption="Requests being served"
              trend={inWindow.map((snapshot) => snapshot.inFlight)}
            />
            <Stat
              label="DB pool"
              value={latest ? `${latest.db.active} / ${latest.db.total}` : "—"}
              caption={latest ? `${latest.db.idle} idle connections` : "Active / pool size"}
              trend={inWindow.map((snapshot) => snapshot.db.active)}
            />
          </section>

          <div className={classes.charts}>
            <section className={classes.panel} aria-label="Throughput">
              <header className={classes.panelHeader}>
                <h2 className={classes.panelTitle}>Throughput</h2>
                <span className={classes.panelCaption}>requests per second</span>
              </header>
              <Chart option={throughput} height={CHART_HEIGHT} notMerge aria-label="Requests per second by status" />
            </section>
            <section className={classes.panel} aria-label="Latency">
              <header className={classes.panelHeader}>
                <h2 className={classes.panelTitle}>Latency</h2>
                {selected ? (
                  <>
                    <span className={classes.panelRoute}>
                      {selected.method} {selected.path}
                    </span>
                    <button
                      type="button"
                      className={classes.clear}
                      onClick={() => setSelectedKey(null)}
                      aria-label="Show all routes"
                      title="Show all routes"
                    >
                      <X size={12} />
                    </button>
                  </>
                ) : (
                  <span className={classes.panelCaption}>
                    {hasHistogram ? "all routes" : "mean of all routes"} · pick a route to narrow it
                  </span>
                )}
              </header>
              <Chart option={latency} height={CHART_HEIGHT} notMerge aria-label="Latency over time" />
            </section>
          </div>

          <section className={classes.panel} aria-label="Routes">
            <header className={classes.panelHeader}>
              <h2 className={classes.panelTitle}>Routes</h2>
              <span className={classes.panelCaption}>
                {inWindow.length > 1 ? `over ${windowLabel}` : "since server start"}
              </span>
              {idleCount > 0 && (
                <Button
                  className={classes.headerAction}
                  variant="subtle"
                  size="sm"
                  onClick={() => setShowIdle((value) => !value)}
                >
                  {showIdle ? "Hide idle routes" : `Show ${idleCount} idle`}
                </Button>
              )}
            </header>
            <DataTable
              aria-label="Routes"
              data={showIdle ? routes : busy}
              columns={COLUMNS}
              getRowId={(route) => route.key}
              sorting={sorting}
              onSortingChange={setSorting}
              activeRowId={selectedKey ?? undefined}
              loading={feed.isPending && snapshots.length === 0}
              loadingRowCount={6}
              maxHeight="none"
              onRowClick={(route) => setSelectedKey((key) => (key === route.key ? null : route.key))}
              rowMenu={(route) => [
                [
                  {
                    text: "Show latency",
                    iconStart: <Gauge size={14} />,
                    action: () => setSelectedKey(route.key),
                  },
                  ...(route.method === "GET" && !route.path.includes("{")
                    ? [
                        {
                          text: "Open in Requests",
                          iconStart: <Send size={14} />,
                          action: () => navigate(`/console?${new URLSearchParams({ url: route.path }).toString()}`),
                        },
                      ]
                    : []),
                ],
              ]}
              emptyState={
                <EmptyState
                  image={<Gauge width={32} height={32} aria-hidden="true" />}
                  title="No traffic yet"
                  description="Routes appear here once the server has served a request."
                />
              }
            />
          </section>
        </>
      )}
    </WorkspacePageLayout>
  );
}
