import { type CSSProperties, type PointerEvent, useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import {
  compact,
  type CursorSync,
  GRID_COLUMNS,
  type GridPos,
  type Panel,
  type Variables,
  visibleLayout,
} from "@/entities/metrics";
import classes from "./MetricsDashboard.module.css";
import { PanelView } from "./PanelView";

/** Row height and gap in px; a panel of height h spans h rows. Matches the CSS. */
export const ROW_PX = 30;
export const GAP_PX = 8;
const MIN_W = 2;
const MIN_H = 3;

export interface DashboardGridProps {
  panels: Panel[];
  range: number;
  end: number;
  variables: Variables;
  editing?: boolean;
  onPanelsChange?: (panels: Panel[]) => void;
  onEditPanel?: (panel: Panel) => void;
  onDuplicatePanel?: (panel: Panel) => void;
  onRemovePanel?: (panel: Panel) => void;
  onViewPanel?: (panel: Panel) => void;
  onInspectPanel?: (panel: Panel) => void;
  /** Drag-to-select on any chart, in epoch ms. */
  onZoom?: (fromMs: number, toMs: number) => void;
  cursorSync?: CursorSync;
}

interface Gesture {
  id: string;
  mode: "move" | "resize";
  startX: number;
  startY: number;
  origin: GridPos;
  column: number;
}

/**
 * Panels on a 24-column grid. In edit mode a panel moves by its header and resizes from its
 * corner, snapping to cells; other panels make room and float up, as in Grafana.
 */
export function DashboardGrid({
  panels,
  range,
  end,
  variables,
  editing,
  onPanelsChange,
  onEditPanel,
  onDuplicatePanel,
  onRemovePanel,
  onViewPanel,
  onInspectPanel,
  onZoom,
  cursorSync = "crosshair",
}: DashboardGridProps) {
  // Charts of one dashboard share the cursor unless sync is off.
  const groupId = `dashboard-${useId()}`;
  const group = cursorSync === "off" ? undefined : groupId;
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const toggleRow = useCallback(
    (id: string) =>
      setCollapsed((current) => {
        const next = new Set(current);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      }),
    []
  );
  const gridRef = useRef<HTMLDivElement | null>(null);
  const [draft, setDraft] = useState<Panel[] | null>(null);
  const draftRef = useRef<Panel[] | null>(null);
  const gesture = useRef<Gesture | null>(null);
  const dragging = draft != null;
  const laidOut = useMemo(() => (editing ? panels : visibleLayout(panels, collapsed)), [editing, panels, collapsed]);
  const shown = draft ?? laidOut;

  const updateDraft = useCallback((next: Panel[] | null) => {
    draftRef.current = next;
    setDraft(next);
  }, []);

  const start = useCallback(
    (panel: Panel, mode: Gesture["mode"]) => (event: PointerEvent<HTMLElement>) => {
      if (!editing || event.button !== 0 || !gridRef.current) return;
      event.preventDefault();
      const width = gridRef.current.clientWidth;
      gesture.current = {
        id: panel.id,
        mode,
        startX: event.clientX,
        startY: event.clientY,
        origin: panel.gridPos,
        column: (width - GAP_PX * (GRID_COLUMNS - 1)) / GRID_COLUMNS,
      };
      updateDraft(panels);
    },
    [editing, panels, updateDraft]
  );

  useEffect(() => {
    if (!dragging) return;
    const move = (event: globalThis.PointerEvent) => {
      const g = gesture.current;
      if (!g) return;
      const dx = Math.round((event.clientX - g.startX) / (g.column + GAP_PX));
      const dy = Math.round((event.clientY - g.startY) / (ROW_PX + GAP_PX));
      const pos: GridPos =
        g.mode === "move"
          ? {
              ...g.origin,
              x: Math.max(0, Math.min(GRID_COLUMNS - g.origin.w, g.origin.x + dx)),
              y: Math.max(0, g.origin.y + dy),
            }
          : {
              ...g.origin,
              w: Math.max(MIN_W, Math.min(GRID_COLUMNS - g.origin.x, g.origin.w + dx)),
              h: Math.max(MIN_H, g.origin.h + dy),
            };
      // Section rows always span the grid.
      if (panels.find((panel) => panel.id === g.id)?.type === "row") {
        pos.x = 0;
        pos.w = GRID_COLUMNS;
        pos.h = 1;
      }
      updateDraft(compact(panels.map((panel) => (panel.id === g.id ? { ...panel, gridPos: pos } : panel)), g.id));
    };
    const finish = () => {
      const result = draftRef.current;
      gesture.current = null;
      updateDraft(null);
      if (result) onPanelsChange?.(compact(result));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
    };
  }, [dragging, panels, onPanelsChange, updateDraft]);

  return (
    <div ref={gridRef} className={classes.grid} data-editing={editing ? "" : undefined} data-dragging={dragging ? "" : undefined}>
      {shown.map((panel) => {
        const style = {
          "--col": `${panel.gridPos.x + 1} / span ${panel.gridPos.w}`,
          "--row": `${panel.gridPos.y + 1} / span ${panel.gridPos.h}`,
          "--h": panel.gridPos.h,
        } as CSSProperties;
        return (
          <div
            key={panel.id}
            className={classes.cell}
            style={style}
            data-active={gesture.current?.id === panel.id ? "" : undefined}
            data-row={panel.type === "row" ? "" : undefined}
          >
            <PanelView
              panel={panel}
              range={range}
              end={end}
              variables={variables}
              group={group}
              sharedTooltip={cursorSync === "tooltip"}
              editing={editing}
              collapsed={!editing && collapsed.has(panel.id)}
              onToggleRow={editing ? undefined : () => toggleRow(panel.id)}
              onEdit={onEditPanel ? () => onEditPanel(panel) : undefined}
              onDuplicate={onDuplicatePanel ? () => onDuplicatePanel(panel) : undefined}
              onRemove={onRemovePanel ? () => onRemovePanel(panel) : undefined}
              onView={onViewPanel ? () => onViewPanel(panel) : undefined}
              onInspect={onInspectPanel ? () => onInspectPanel(panel) : undefined}
              onZoom={editing ? undefined : onZoom}
              onDragStart={start(panel, "move")}
              onResizeStart={panel.type === "row" ? undefined : start(panel, "resize")}
            />
          </div>
        );
      })}
    </div>
  );
}
