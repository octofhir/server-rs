import {
  Alert,
  Button,
  ClipboardButton,
  Drawer,
  DropdownMenu,
  EmptyState,
  Modal,
  SegmentedRadioGroup,
  Select,
  TextInput,
} from "@octofhir/ui-kit";
import {
  Copy,
  FileJson,
  Gauge,
  LayoutDashboard,
  ListPlus,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
  Upload,
  X,
  ZoomOut,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";
import {
  bottomRow,
  CURSOR_SYNC_OPTIONS,
  type CursorSync,
  type Dashboard,
  emptyPanel,
  emptyRow,
  newPanelId,
  type Panel,
  resolveWindow,
  TIME_RANGES,
  useBuiltinDashboards,
  useDeleteDashboard,
  useMetricsStatus,
  useSaveDashboard,
  useSavedDashboards,
  variableValues,
  windowLabel,
} from "@/entities/metrics";
import { useSettings } from "@/shared/api/hooks/useSystemQueries";
import { usePaletteContribution } from "@/shared/command-palette";
import { DashboardGrid, PanelEditor, PanelView } from "@/widgets/metrics-dashboard";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { GrafanaExport } from "./GrafanaExport";
import classes from "./PerformancePage.module.css";
import { VariablePicker } from "./VariablePicker";

const DEFAULT_DASHBOARD = "system";
const MIN_REFRESH_SECS = 5;

const REFRESH_OPTIONS = [
  { value: "auto", label: "Auto" },
  { value: "off", label: "Off" },
  { value: "15", label: "15s" },
  { value: "30", label: "30s" },
  { value: "60", label: "1m" },
  { value: "300", label: "5m" },
];

const CONFIG_SNIPPET = `[metrics.prometheus]
url = "http://prometheus:9090"
selector = '{job="octofhir"}'
scrape_interval_secs = 15`;

function nowSeconds(step: number): number {
  return Math.floor(Date.now() / 1000 / step) * step;
}

/** Ticks every `step` seconds while live, aligned so every panel shares one end time. */
function useRefreshClock(step: number, live: boolean): [number, () => void] {
  const [end, setEnd] = useState(() => nowSeconds(step));
  useEffect(() => {
    setEnd(nowSeconds(step));
    if (!live) return;
    const timer = window.setInterval(() => setEnd(nowSeconds(step)), step * 1000);
    return () => window.clearInterval(timer);
  }, [step, live]);
  const refresh = useCallback(() => setEnd(Math.floor(Date.now() / 1000)), []);
  return [end, refresh];
}

const CURSOR_SYNC_KEY = "octofhir.metrics.cursorSync";

/** The viewer's own cursor-sync choice per dashboard; overrides the dashboard's default. */
function readCursorSync(id: string | undefined): CursorSync | null {
  try {
    const stored = JSON.parse(window.localStorage.getItem(CURSOR_SYNC_KEY) ?? "{}") as Record<string, CursorSync>;
    return id ? (stored[id] ?? null) : null;
  } catch {
    return null;
  }
}

function writeCursorSync(id: string, value: CursorSync): void {
  try {
    const stored = JSON.parse(window.localStorage.getItem(CURSOR_SYNC_KEY) ?? "{}") as Record<string, CursorSync>;
    window.localStorage.setItem(CURSOR_SYNC_KEY, JSON.stringify({ ...stored, [id]: value }));
  } catch {
    // Storage unavailable: the choice lasts until the page reloads.
  }
}

function downloadJson(name: string, data: unknown): void {
  const blob = new Blob([`${JSON.stringify(data, null, 2)}\n`], { type: "application/json" });
  const href = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = href;
  link.download = name;
  link.click();
  URL.revokeObjectURL(href);
}

function InspectQuery({ panel, onClose }: { panel: Panel; onClose: () => void }) {
  return (
    <Modal open onClose={onClose} title={`${panel.title} — queries`} size="lg">
      <div className={classes.inspect}>
        {panel.targets.map((target) => (
          <section key={target.refId} className={classes.inspectQuery}>
            <header className={classes.inspectHead}>
              <span className={classes.refId}>{target.refId}</span>
              {target.legend ? <span className={classes.inspectLegend}>{target.legend}</span> : null}
              <ClipboardButton text={target.expr} size="sm" variant="subtle" />
            </header>
            <pre className={classes.code}>{target.expr}</pre>
          </section>
        ))}
        <p className={classes.inspectNote}>
          The server adds this deployment's selector to every series selector and fills in{" "}
          <code>$__rate_interval</code>, <code>$__range</code> and dashboard variables before sending it to Prometheus.
        </p>
      </div>
    </Modal>
  );
}

export function PerformancePage() {
  const navigate = useNavigate();
  const { id: routeId } = useParams<{ id: string }>();
  const [params, setParams] = useSearchParams();
  const settings = useSettings();
  const enabled = settings.data?.features.metrics === true;

  const status = useMetricsStatus(enabled);
  const builtin = useBuiltinDashboards(enabled);
  const saved = useSavedDashboards(enabled);
  const save = useSaveDashboard();
  const remove = useDeleteDashboard();

  const dashboards = useMemo(
    () => [
      ...(builtin.data ?? []).map((d) => ({ dashboard: d, builtin: true })),
      ...(saved.data ?? []).map((d) => ({ dashboard: d, builtin: false })),
    ],
    [builtin.data, saved.data]
  );
  const currentId = routeId ?? DEFAULT_DASHBOARD;
  const current = dashboards.find((entry) => entry.dashboard.id === currentId) ?? null;

  const [editing, setEditing] = useState<Dashboard | null>(null);
  const [editingPanel, setEditingPanel] = useState<Panel | null>(null);
  const [viewing, setViewing] = useState<Panel | null>(null);
  const [inspecting, setInspecting] = useState<Panel | null>(null);
  const [exporting, setExporting] = useState(false);
  const [deleting, setDeleting] = useState(false);
  // A dashboard just created or duplicated opens in edit mode once its route is current.
  const editAfterOpen = useRef<Dashboard | null>(null);
  const shown = editing ?? current?.dashboard ?? null;

  useEffect(() => {
    const pending = editAfterOpen.current;
    editAfterOpen.current = null;
    setEditing(pending?.id === currentId ? pending : null);
  }, [currentId]);

  // Time: relative (`now-1h`) follows the clock; absolute (after a zoom) stays put.
  const defaultFrom = shown?.time?.from ?? "now-1h";
  const from = params.get("from") ?? defaultFrom;
  const to = params.get("to");
  const timeWindow = resolveWindow(from, to);
  const zoomed = timeWindow.end != null;

  const scrape = Math.max(MIN_REFRESH_SECS, status.data?.scrapeIntervalSecs ?? 15);
  const refreshSetting = params.get("refresh") ?? "auto";
  const refreshSecs = refreshSetting === "auto" ? scrape : Number(refreshSetting) || scrape;
  const live = refreshSetting !== "off" && !zoomed && !editingPanel;
  const [clockEnd, refresh] = useRefreshClock(refreshSecs, live);
  const end = timeWindow.end ?? clockEnd;
  const range = timeWindow.range;

  const [viewerSync, setViewerSync] = useState<CursorSync | null>(null);
  useEffect(() => setViewerSync(readCursorSync(currentId)), [currentId]);
  const cursorSync: CursorSync = editing
    ? (editing.cursorSync ?? "crosshair")
    : (viewerSync ?? shown?.cursorSync ?? "crosshair");
  const changeCursorSync = useCallback(
    (value: CursorSync) => {
      if (editing) {
        setEditing({ ...editing, cursorSync: value });
        return;
      }
      setViewerSync(value);
      writeCursorSync(currentId, value);
    },
    [editing, currentId]
  );

  const variableDefs = shown?.variables ?? [];
  const selection = useMemo(() => {
    const out: Record<string, string[]> = {};
    for (const variable of variableDefs) {
      const raw = params.get(`var-${variable.name}`);
      out[variable.name] = raw ? raw.split(",").filter(Boolean) : [];
    }
    return out;
  }, [params, variableDefs]);
  const variables = useMemo(() => variableValues(variableDefs, selection), [variableDefs, selection]);
  const variableNames = useMemo(() => variableDefs.map((v) => v.name), [variableDefs]);

  const setParam = useCallback(
    (patch: Record<string, string | null>, push = false) =>
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          for (const [key, value] of Object.entries(patch)) {
            if (value) next.set(key, value);
            else next.delete(key);
          }
          return next;
        },
        { replace: !push }
      ),
    [setParams]
  );

  // Connected charts all report the same zoom; apply it once.
  const lastZoom = useRef("");
  const zoom = useCallback(
    (fromMs: number, toMs: number) => {
      const key = `${fromMs}-${toMs}`;
      if (lastZoom.current === key) return;
      lastZoom.current = key;
      setParam({ from: String(fromMs), to: String(toMs) }, true);
    },
    [setParam]
  );
  const setRelative = useCallback(
    (value: string) => {
      lastZoom.current = "";
      setParam({ from: value === defaultFrom ? null : value, to: null });
    },
    [setParam, defaultFrom]
  );
  const zoomOut = useCallback(() => {
    const span = range * 2 * 1000;
    const centre = (end - range / 2) * 1000;
    const toMs = Math.min(Date.now(), centre + span / 2);
    zoom(Math.round(toMs - span), Math.round(toMs));
  }, [end, range, zoom]);

  const open = useCallback((id: string) => navigate(`/performance/${encodeURIComponent(id)}`), [navigate]);

  const duplicate = useCallback(async () => {
    if (!shown) return;
    const { id: _id, ...rest } = shown;
    const copy = await save.mutateAsync({ ...rest, title: `${shown.title} (copy)` });
    if (copy.id) {
      editAfterOpen.current = copy;
      open(copy.id);
    }
  }, [shown, save, open]);

  const create = useCallback(async () => {
    const created = await save.mutateAsync({
      title: "New dashboard",
      time: { from: "now-1h", to: "now" },
      variables: [{ name: "instance", label: "instance", labelName: "instance", metric: "up", multi: true, includeAll: true }],
      panels: [],
    });
    if (created.id) {
      editAfterOpen.current = created;
      open(created.id);
    }
  }, [save, open]);

  const commit = useCallback(async () => {
    if (!editing) return;
    await save.mutateAsync({ ...editing, title: editing.title.trim() || "Untitled dashboard" });
    setEditing(null);
  }, [editing, save]);

  const updatePanels = useCallback((panels: Panel[]) => setEditing((d) => (d ? { ...d, panels } : d)), []);

  const addPanel = useCallback(() => {
    if (!editing) return;
    const panel = emptyPanel(editing.panels);
    updatePanels([...editing.panels, panel]);
    setEditingPanel(panel);
  }, [editing, updatePanels]);

  const addRow = useCallback(() => {
    if (!editing) return;
    const row = emptyRow(editing.panels);
    updatePanels([...editing.panels, row]);
    setEditingPanel(row);
  }, [editing, updatePanels]);

  usePaletteContribution(
    "performance",
    useMemo(
      () =>
        enabled
          ? [
              ...TIME_RANGES.map((option) => ({
                id: `performance-range-${option.value}`,
                label: `Show the last ${option.label}`,
                group: "Performance",
                icon: Gauge,
                onSelect: () => setRelative(option.value),
              })),
              ...(zoomed
                ? [{ id: "performance-zoom-out", label: "Zoom out", group: "Performance", icon: ZoomOut, onSelect: zoomOut }]
                : []),
              ...dashboards.map((entry) => ({
                id: `performance-open-${entry.dashboard.id}`,
                label: `Open dashboard ${entry.dashboard.title}`,
                group: "Performance",
                icon: LayoutDashboard,
                onSelect: () => entry.dashboard.id && open(entry.dashboard.id),
              })),
              { id: "performance-refresh", label: "Refresh dashboard now", group: "Performance", icon: RefreshCw, onSelect: refresh },
              { id: "performance-new", label: "New dashboard", group: "Performance", icon: Plus, onSelect: create },
              ...(shown
                ? [
                    {
                      id: "performance-export",
                      label: "Export dashboard to Grafana",
                      group: "Performance",
                      icon: Upload,
                      onSelect: () => setExporting(true),
                    },
                  ]
                : []),
            ]
          : [],
      [enabled, dashboards, zoomed, shown, setRelative, zoomOut, refresh, open, create]
    )
  );

  if (settings.isSuccess && !enabled) {
    return (
      <WorkspacePageLayout title="Performance" className="page-enter">
        <EmptyState
          size="promo"
          image={<Gauge width={32} height={32} aria-hidden="true" />}
          title="Connect Prometheus to see dashboards"
          description="Dashboards read this deployment's metrics from Prometheus, so every instance behind a load balancer is counted. Add the server's Prometheus to octofhir.toml and restart:"
        />
        <pre className={classes.snippet}>{CONFIG_SNIPPET}</pre>
      </WorkspacePageLayout>
    );
  }

  const targets = status.data?.targets ?? [];
  const upCount = targets.filter((t) => t.up).length;
  const versions = [...new Set(targets.map((t) => t.version).filter(Boolean))];

  const deletable = current != null && !current.builtin && current.dashboard.id != null;

  const exportMenu = [
    { text: "Export to Grafana…", iconStart: <Upload size={14} />, action: () => setExporting(true) },
    {
      text: "Download dashboard JSON",
      iconStart: <FileJson size={14} />,
      action: () => shown && downloadJson(`${(shown.id ?? "dashboard").replace(/[^a-zA-Z0-9-_]/g, "-")}.dashboard.json`, shown),
    },
  ];

  const moreMenu = [
    { text: current?.builtin ? "Duplicate & edit" : "Duplicate", iconStart: <Copy size={14} />, action: duplicate },
    ...(deletable
      ? [[{ text: "Delete dashboard…", iconStart: <Trash2 size={14} />, theme: "danger" as const, action: () => setDeleting(true) }]]
      : []),
  ];

  const statusState = status.data?.reachable === false ? "error" : !live ? "paused" : "live";
  const statusText =
    status.data?.reachable === false
      ? "Prometheus unreachable"
      : `${upCount}/${targets.length} up${versions.length ? ` · v${versions.join(", v")}` : ""}`;

  return (
    <WorkspacePageLayout
      title="Performance"
      description={shown?.description ?? "This deployment's metrics, read from Prometheus."}
      className="page-enter"
      actions={
        editing ? (
          <>
            <Button variant="outline" size="sm" onClick={addRow}>
              <Button.Icon side="left">
                <ListPlus size={14} />
              </Button.Icon>
              Add section
            </Button>
            <Button variant="outline" size="sm" onClick={addPanel}>
              <Button.Icon side="left">
                <Plus size={14} />
              </Button.Icon>
              Add panel
            </Button>
            {deletable ? (
              <Button variant="subtle" size="sm" color="fire" onClick={() => setDeleting(true)}>
                <Button.Icon side="left">
                  <Trash2 size={14} />
                </Button.Icon>
                Delete
              </Button>
            ) : null}
            <Button variant="subtle" size="sm" onClick={() => setEditing(null)}>
              Cancel
            </Button>
            <Button color="primary" size="sm" onClick={commit} loading={save.isPending}>
              Save
            </Button>
          </>
        ) : (
          <>
            {current?.builtin ? (
              <Button variant="outline" size="sm" onClick={duplicate} loading={save.isPending}>
                <Button.Icon side="left">
                  <Copy size={14} />
                </Button.Icon>
                Duplicate &amp; edit
              </Button>
            ) : current ? (
              <Button variant="outline" size="sm" onClick={() => setEditing(current.dashboard)}>
                <Button.Icon side="left">
                  <Pencil size={14} />
                </Button.Icon>
                Edit
              </Button>
            ) : null}
            <Button variant="outline" size="sm" onClick={create}>
              <Button.Icon side="left">
                <Plus size={14} />
              </Button.Icon>
              New
            </Button>
            {shown ? (
              <DropdownMenu
                items={exportMenu}
                icon={<Upload size={14} />}
                defaultSwitcherProps={{ variant: "outline", size: "sm", "aria-label": "Export" }}
              />
            ) : null}
            {shown ? (
              <DropdownMenu
                items={moreMenu}
                defaultSwitcherProps={{ variant: "outline", size: "sm", "aria-label": "More dashboard actions" }}
              />
            ) : null}
          </>
        )
      }
    >
      <div className={classes.toolbar}>
        <div className={classes.toolbarGroup}>
          {editing ? (
            <TextInput
              aria-label="Dashboard title"
              value={editing.title}
              onChange={(title) => setEditing({ ...editing, title })}
              size="sm"
              w={260}
            />
          ) : (
            <Select
              aria-label="Dashboard"
              size="sm"
              w={220}
              value={currentId}
              onChange={(value) => value && open(value)}
              data={[
                ...(builtin.data?.length
                  ? [{ group: "Built-in", items: builtin.data.map((d) => ({ value: d.id ?? "", label: d.title })) }]
                  : []),
                ...(saved.data?.length
                  ? [{ group: "Saved", items: saved.data.map((d) => ({ value: d.id ?? "", label: d.title })) }]
                  : []),
              ]}
            />
          )}
          {variableDefs.map((variable) => (
            <VariablePicker
              key={variable.name}
              variable={variable}
              value={selection[variable.name] ?? []}
              onChange={(value) => setParam({ [`var-${variable.name}`]: value.length ? value.join(",") : null })}
            />
          ))}
        </div>
        <div className={classes.toolbarGroup}>
          <span className={classes.live} data-state={statusState} title={status.data?.selector ?? undefined}>
            <span className={classes.liveDot} aria-hidden="true" />
            {statusText}
          </span>
          {zoomed ? (
            <span className={classes.zoomChip}>
              <span>{windowLabel((end - range) * 1000, end * 1000)}</span>
              <button type="button" onClick={zoomOut} aria-label="Zoom out" title="Zoom out">
                <ZoomOut size={13} />
              </button>
              <button type="button" onClick={() => setRelative(defaultFrom)} aria-label="Back to live" title="Back to live">
                <X size={13} />
              </button>
            </span>
          ) : null}
          <SegmentedRadioGroup
            aria-label="Time range"
            size="sm"
            value={zoomed ? "" : from}
            onChange={setRelative}
            options={TIME_RANGES.map((option) => ({ value: option.value, label: option.label }))}
          />
          <Select
            aria-label="Cursor sync between charts"
            size="sm"
            w={156}
            value={cursorSync}
            onChange={(value) => value && changeCursorSync(value as CursorSync)}
            data={CURSOR_SYNC_OPTIONS}
          />
          <Select
            aria-label="Auto-refresh"
            size="sm"
            w={92}
            value={refreshSetting}
            onChange={(value) => setParam({ refresh: !value || value === "auto" ? null : value })}
            data={REFRESH_OPTIONS.map((option) =>
              option.value === "auto" ? { ...option, label: `${scrape}s` } : option
            )}
          />
          <Button variant="outline" size="sm" onClick={refresh} aria-label="Refresh now" title="Refresh now">
            <RefreshCw size={14} />
          </Button>
        </div>
      </div>

      {status.data && !status.data.reachable ? (
        <Alert theme="danger" title="Prometheus is unreachable" message={status.data.error ?? "No answer"} />
      ) : status.data && targets.length === 0 ? (
        <Alert
          theme="warning"
          title="Prometheus does not scrape this server"
          message={`No targets match ${status.data.selector}. Add a scrape job for this server's /metrics with that job label.`}
        />
      ) : null}

      {shown ? (
        shown.panels.length === 0 && !editing ? (
          <EmptyState
            image={<LayoutDashboard width={32} height={32} aria-hidden="true" />}
            title="No panels yet"
            description="Edit the dashboard to add PromQL panels."
            actions={current && !current.builtin ? [{ text: "Edit", onClick: () => setEditing(current.dashboard) }] : []}
          />
        ) : (
          <DashboardGrid
            panels={shown.panels}
            range={range}
            end={end}
            variables={variables}
            editing={editing != null}
            onPanelsChange={updatePanels}
            onZoom={zoom}
            cursorSync={cursorSync}
            onViewPanel={setViewing}
            onInspectPanel={setInspecting}
            onEditPanel={editing ? setEditingPanel : undefined}
            onDuplicatePanel={
              editing
                ? (panel) =>
                    updatePanels([
                      ...editing.panels,
                      { ...panel, id: newPanelId(), gridPos: { ...panel.gridPos, y: bottomRow(editing.panels) } },
                    ])
                : undefined
            }
            onRemovePanel={editing ? (panel) => updatePanels(editing.panels.filter((p) => p.id !== panel.id)) : undefined}
          />
        )
      ) : builtin.isSuccess && saved.isSuccess ? (
        <EmptyState
          image={<LayoutDashboard width={32} height={32} aria-hidden="true" />}
          title="Dashboard not found"
          description={`There is no dashboard “${currentId}”.`}
          actions={[{ text: "Open System", onClick: () => open(DEFAULT_DASHBOARD) }]}
        />
      ) : null}

      {editingPanel && editing ? (
        <PanelEditor
          panel={editingPanel}
          onClose={() => setEditingPanel(null)}
          onApply={(panel) => {
            updatePanels(editing.panels.map((p) => (p.id === panel.id ? panel : p)));
            setEditingPanel(null);
          }}
          range={range}
          end={end}
          variables={variables}
          variableNames={variableNames}
        />
      ) : null}
      {viewing ? (
        <Drawer open onClose={() => setViewing(null)} placement="right" size="min(1280px, 96vw)" title={viewing.title}>
          <div className={classes.viewBody}>
            <PanelView
              panel={viewing}
              range={range}
              end={end}
              variables={variables}
              onZoom={zoom}
              onInspect={() => setInspecting(viewing)}
            />
          </div>
        </Drawer>
      ) : null}
      {inspecting ? <InspectQuery panel={inspecting} onClose={() => setInspecting(null)} /> : null}
      {exporting && shown ? <GrafanaExport dashboard={shown} onClose={() => setExporting(false)} /> : null}
      {deleting && current?.dashboard.id ? (
        <Modal
          open
          onClose={() => setDeleting(false)}
          title="Delete dashboard"
          size="sm"
          footer={
            <>
              <Button variant="subtle" onClick={() => setDeleting(false)}>
                Cancel
              </Button>
              <Button
                color="fire"
                loading={remove.isPending}
                onClick={async () => {
                  if (!current.dashboard.id) return;
                  await remove.mutateAsync(current.dashboard.id);
                  setDeleting(false);
                  setEditing(null);
                  open(DEFAULT_DASHBOARD);
                }}
              >
                Delete
              </Button>
            </>
          }
        >
          <p className={classes.confirmText}>
            Delete <b>{current.dashboard.title}</b>? Its history stays in the <code>Dashboard</code> resource history,
            but the dashboard disappears for everyone.
          </p>
          {remove.error ? <Alert theme="danger" message={remove.error instanceof Error ? remove.error.message : "Delete failed"} /> : null}
        </Modal>
      ) : null}
    </WorkspacePageLayout>
  );
}
