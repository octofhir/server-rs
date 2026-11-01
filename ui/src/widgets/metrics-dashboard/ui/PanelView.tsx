import {
  Chart,
  DataTable,
  type DataTableColumn,
  type DataTableSort,
  DropdownMenu,
  type EChartsType,
  Tooltip,
  useColorScheme,
} from "@octofhir/ui-kit";
import { ChevronDown, ChevronRight, CircleAlert, Code, Copy, GripVertical, Info, Maximize2, Pencil, Trash2 } from "lucide-react";
import { type MouseEvent, type PointerEvent, useMemo, useRef, useState } from "react";
import {
  formatValue,
  groupingOrder,
  joinTable,
  lastValue,
  MetricsApiError,
  type Panel,
  type TableRow,
  thresholdColor,
  usePanelData,
  type Variables,
} from "@/entities/metrics";
import { type DrawnSeries, drawnSeries, semanticColor, statTrendOption, timeseriesOption } from "./chartOption";
import classes from "./MetricsDashboard.module.css";

export interface PanelViewProps {
  panel: Panel;
  /** Seconds the dashboard shows, ending at `end` (unix seconds). */
  range: number;
  end: number;
  variables: Variables;
  /** Charts of one group share the cursor. */
  group?: string;
  /** With a group: every chart shows its values box, not only the hovered one. */
  sharedTooltip?: boolean;
  editing?: boolean;
  /** Rows only. */
  collapsed?: boolean;
  onToggleRow?: () => void;
  onEdit?: () => void;
  onDuplicate?: () => void;
  onRemove?: () => void;
  onView?: () => void;
  onInspect?: () => void;
  /** Drag-to-select on a chart, in epoch ms. */
  onZoom?: (fromMs: number, toMs: number) => void;
  onDragStart?: (event: PointerEvent<HTMLElement>) => void;
  onResizeStart?: (event: PointerEvent<HTMLElement>) => void;
}

function errorText(error: unknown): string {
  if (error instanceof MetricsApiError) return error.message;
  return error instanceof Error ? error.message : "The query failed";
}

type Data = ReturnType<typeof usePanelData>["data"];

function StatBody({ panel, data }: { panel: Panel; data: Data }) {
  const { colorScheme } = useColorScheme();
  const series = data?.ranges?.[0]?.result.series ?? [];
  const value = lastValue(series);
  const tone = thresholdColor(panel.thresholds, value);
  const points = series[0]?.points ?? [];
  const color = semanticColor(tone ?? "accent", colorScheme);
  const option = useMemo(() => statTrendOption(points, color), [points, color]);
  const formatted = formatValue(value, panel.unit, panel.decimals);
  // Split "12.3 ms" into number and unit so the unit can be quieter.
  const match = /^(-?[\d.,]+)\s*(.*)$/.exec(formatted);
  return (
    <div className={classes.stat} data-tone={tone ?? undefined}>
      <span className={classes.statValue}>
        {match ? (
          <>
            {match[1]}
            {match[2] ? <span className={classes.statUnit}>{match[2]}</span> : null}
          </>
        ) : (
          formatted
        )}
      </span>
      {points.length > 1 ? (
        <div className={classes.statTrend}>
          <Chart option={option} height="100%" notMerge aria-label={`${panel.title} trend`} />
        </div>
      ) : null}
    </div>
  );
}

function TableBody({ panel, data }: { panel: Panel; data: Data }) {
  const { labelColumns, rows } = useMemo(
    () => joinTable(data?.instants ?? [], groupingOrder(panel.targets[0]?.expr)),
    [data, panel.targets]
  );
  const columns = useMemo<DataTableColumn<TableRow>[]>(
    () => [
      ...labelColumns.map(
        (label): DataTableColumn<TableRow> => ({
          id: `label:${label}`,
          header: label,
          sortable: true,
          accessor: (row) => row.labels[label] ?? "",
          cell: (row) => <span className={classes.mono}>{row.labels[label] ?? ""}</span>,
        })
      ),
      ...panel.targets.map(
        (target): DataTableColumn<TableRow> => ({
          id: `value:${target.refId}`,
          header: target.legend || target.refId,
          align: "right",
          width: 104,
          sortable: true,
          accessor: (row) => row.values[target.refId] ?? -Infinity,
          cell: (row) => {
            const value = row.values[target.refId];
            return value == null || Number.isNaN(value) ? (
              <span className={classes.muted}>—</span>
            ) : (
              <span className={classes.num}>{formatValue(value, target.unit ?? panel.unit, panel.decimals)}</span>
            );
          },
        })
      ),
    ],
    [labelColumns, panel]
  );
  const firstValue = panel.targets[0] ? `value:${panel.targets[0].refId}` : undefined;
  const [sorting, setSorting] = useState<DataTableSort[]>(firstValue ? [{ id: firstValue, desc: true }] : []);
  return (
    <div className={classes.table}>
      <DataTable
        aria-label={panel.title}
        data={rows}
        columns={columns}
        getRowId={(row) => row.key}
        sorting={sorting}
        onSortingChange={setSorting}
        stickyHeader
        maxHeight="100%"
        emptyState={<span className={classes.muted}>No data in this range</span>}
      />
    </div>
  );
}

function Legend({
  panel,
  series,
  hidden,
  onToggle,
}: {
  panel: Panel;
  series: DrawnSeries[];
  hidden: ReadonlySet<string>;
  onToggle: (key: string, additive: boolean) => void;
}) {
  if (series.length < 2) return null;
  return (
    <ul className={classes.legend} aria-label="Series">
      {series.map((s) => (
        <li key={s.key}>
          <button
            type="button"
            className={classes.legendItem}
            data-off={hidden.has(s.key) ? "" : undefined}
            onClick={(event: MouseEvent) => onToggle(s.key, event.metaKey || event.ctrlKey || event.shiftKey)}
            title="Click to show only this series; ⌘-click to toggle it"
          >
            <span className={classes.swatch} style={{ background: s.color }} aria-hidden="true" />
            <span className={classes.legendName}>{s.name}</span>
            <span className={classes.legendValue}>{formatValue(s.last, panel.unit, panel.decimals)}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

function TimeseriesBody({
  panel,
  data,
  range,
  end,
  group,
  sharedTooltip,
  onZoom,
}: {
  panel: Panel;
  data: Data;
  range: number;
  end: number;
  group?: string;
  sharedTooltip?: boolean;
  onZoom?: (fromMs: number, toMs: number) => void;
}) {
  const { colorScheme } = useColorScheme();
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  // Charts of a group share the crosshair; the values box shows only on the hovered one.
  const [hovered, setHovered] = useState(false);
  const series = useMemo(() => drawnSeries(panel, data?.ranges ?? [], colorScheme), [panel, data, colorScheme]);
  const option = useMemo(
    () =>
      timeseriesOption(panel, series, { from: (end - range) * 1000, to: end * 1000 }, hidden, {
        zoom: onZoom != null,
        showTooltip: group == null || sharedTooltip || hovered,
      }),
    [panel, series, end, range, hidden, onZoom, hovered, group, sharedTooltip]
  );

  // Drag-to-select is a toolbox cursor that every (lazy, notMerge) setOption resets, so it is
  // re-armed once each new option has rendered.
  const optionRef = useRef(option);
  optionRef.current = option;
  const armedFor = useRef<unknown>(null);

  const events = useMemo(
    () =>
      onZoom
        ? {
            finished: (_: unknown, chart: EChartsType) => {
              if (armedFor.current === optionRef.current || chart.isDisposed()) return;
              armedFor.current = optionRef.current;
              chart.dispatchAction({ type: "takeGlobalCursor", key: "dataZoomSelect", dataZoomSelectActive: true });
            },
            datazoom: (params: unknown) => {
              const batch = (params as { batch?: { startValue?: number; endValue?: number }[] }).batch?.[0];
              if (batch?.startValue != null && batch.endValue != null && batch.endValue > batch.startValue) {
                onZoom(Math.round(batch.startValue), Math.round(batch.endValue));
              }
            },
          }
        : undefined,
    [onZoom]
  );

  const toggle = (key: string, additive: boolean) =>
    setHidden((current) => {
      if (additive) {
        const next = new Set(current);
        if (next.has(key)) next.delete(key);
        else next.add(key);
        return next;
      }
      const soloed = current.size === series.length - 1 && !current.has(key);
      return soloed ? new Set() : new Set(series.filter((s) => s.key !== key).map((s) => s.key));
    });

  if (data && series.length === 0) return <p className={classes.noData}>No data in this range</p>;
  return (
    <>
      <div
        className={classes.chart}
        onMouseEnter={group ? () => setHovered(true) : undefined}
        onMouseLeave={group ? () => setHovered(false) : undefined}
      >
        <Chart
          option={option}
          height="100%"
          notMerge
          group={group}
          onEvents={events}
          aria-label={panel.title}
        />
      </div>
      <Legend panel={panel} series={series} hidden={hidden} onToggle={toggle} />
    </>
  );
}

/** One panel: header with title and menu, body drawn by panel type. */
export function PanelView({
  panel,
  range,
  end,
  variables,
  group,
  sharedTooltip,
  editing,
  collapsed,
  onToggleRow,
  onEdit,
  onDuplicate,
  onRemove,
  onView,
  onInspect,
  onZoom,
  onDragStart,
  onResizeStart,
}: PanelViewProps) {
  const query = usePanelData(panel, range, end, variables, panel.type !== "row");

  if (panel.type === "row") {
    return (
      <div className={classes.row} data-collapsed={collapsed ? "" : undefined}>
        {editing && onDragStart ? (
          <span className={classes.grip} onPointerDown={onDragStart} aria-hidden="true">
            <GripVertical size={14} />
          </span>
        ) : null}
        <button type="button" className={classes.rowToggle} onClick={onToggleRow} aria-expanded={!collapsed}>
          {collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
          <span className={classes.rowTitle}>{panel.title}</span>
        </button>
        <span className={classes.rowRule} aria-hidden="true" />
        {editing ? (
          <span className={classes.rowActions}>
            <DropdownMenu
              items={[
                ...(onEdit ? [{ text: "Rename", iconStart: <Pencil size={14} />, action: onEdit }] : []),
                ...(onRemove ? [{ text: "Remove section", iconStart: <Trash2 size={14} />, action: onRemove, theme: "danger" as const }] : []),
              ]}
              size="sm"
              defaultSwitcherProps={{ variant: "subtle", size: "sm", "aria-label": `${panel.title} actions` }}
            />
          </span>
        ) : null}
      </div>
    );
  }

  const empty = panel.targets.every((target) => target.expr.trim() === "");
  const menu = [
    ...(onView ? [{ text: "View", iconStart: <Maximize2 size={14} />, action: onView }] : []),
    ...(onInspect ? [{ text: "Query", iconStart: <Code size={14} />, action: onInspect }] : []),
    ...(editing
      ? [
          [
            ...(onEdit ? [{ text: "Edit", iconStart: <Pencil size={14} />, action: onEdit }] : []),
            ...(onDuplicate ? [{ text: "Duplicate", iconStart: <Copy size={14} />, action: onDuplicate }] : []),
            ...(onRemove ? [{ text: "Remove", iconStart: <Trash2 size={14} />, action: onRemove, theme: "danger" as const }] : []),
          ],
        ]
      : []),
  ];

  return (
    <section className={classes.panel} aria-label={panel.title} data-type={panel.type}>
      <header className={classes.panelHeader}>
        {editing && onDragStart ? (
          <span className={classes.grip} onPointerDown={onDragStart} title="Drag to move" aria-hidden="true">
            <GripVertical size={14} />
          </span>
        ) : null}
        <h2
          className={classes.panelTitle}
          onPointerDown={editing ? onDragStart : undefined}
          onDoubleClick={editing ? onEdit : onView}
        >
          {panel.title}
        </h2>
        {panel.description ? (
          <Tooltip label={panel.description} multiline w={260}>
            <span className={classes.icon}>
              <Info size={12} />
            </span>
          </Tooltip>
        ) : null}
        {query.error ? (
          <Tooltip label={errorText(query.error)} multiline w={320}>
            <span className={classes.errorIcon} role="img" aria-label={`Query failed: ${errorText(query.error)}`}>
              <CircleAlert size={13} />
            </span>
          </Tooltip>
        ) : null}
        {query.isFetching ? <span className={classes.fetching} aria-hidden="true" /> : null}
        {menu.length > 0 ? (
          <span className={classes.panelMenu}>
            <DropdownMenu
              items={menu}
              size="sm"
              defaultSwitcherProps={{ variant: "subtle", size: "sm", "aria-label": `${panel.title} actions` }}
            />
          </span>
        ) : null}
      </header>
      <div className={classes.panelBody}>
        {empty ? (
          <button type="button" className={classes.placeholder} onClick={onEdit} disabled={!onEdit}>
            Add a query
          </button>
        ) : query.error && !query.data ? (
          <p className={classes.panelError}>{errorText(query.error)}</p>
        ) : !query.data ? (
          <div className={classes.loading} aria-hidden="true" />
        ) : panel.type === "stat" ? (
          <StatBody panel={panel} data={query.data} />
        ) : panel.type === "table" ? (
          <TableBody panel={panel} data={query.data} />
        ) : (
          <TimeseriesBody
            panel={panel}
            data={query.data}
            range={range}
            end={end}
            group={group}
            sharedTooltip={sharedTooltip}
            onZoom={onZoom}
          />
        )}
      </div>
      {editing && onResizeStart ? (
        <span className={classes.resize} onPointerDown={onResizeStart} title="Drag to resize" aria-hidden="true" />
      ) : null}
    </section>
  );
}
