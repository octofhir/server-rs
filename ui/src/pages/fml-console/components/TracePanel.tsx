/**
 * Replay of a debug trace: the event list, step controls (in, over, out, back), the stack of
 * open scopes, and the variables of the current event grouped by their context type.
 */

import { Badge, Button, EmptyState } from "@octofhir/ui-kit";
import { ArrowDownToLine, ArrowUpFromLine, CornerDownRight, StepBack, StepForward } from "lucide-react";
import { useEffect, useMemo, useRef } from "react";
import classes from "../FmlConsolePage.module.css";
import {
  type DebugResult,
  positionLabel,
  stackAt,
  stepOut,
  stepOver,
  type TraceEvent,
  type TraceVariable,
} from "../trace";

export interface TracePanelProps {
  trace: DebugResult;
  current: number;
  onSelect: (index: number) => void;
  /** Where input paths are in an XML input (CDA), by path. */
  positions?: Record<string, { line: number; column: number }>;
  /** The name of an event in the list; by default, its position in the map. */
  labelOf?: (event: TraceEvent) => string;
}

const STATUS_COLOR: Record<DebugResult["status"], string> = {
  complete: "green",
  truncated: "orange",
  stopped: "red",
};

function valueText(variable: TraceVariable): string {
  if (variable.value === undefined) return "(empty)";
  if (typeof variable.value === "string") return JSON.stringify(variable.value);
  return JSON.stringify(variable.value, null, 2);
}

export function TracePanel({ trace, current, onSelect, positions, labelOf }: TracePanelProps) {
  const events = trace.events;
  const event = events[current];
  const stack = useMemo(() => stackAt(events, current), [events, current]);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>(`[data-index="${current}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [current]);

  const groups = useMemo(() => {
    const byContext = new Map<string, TraceVariable[]>();
    for (const variable of event?.variables ?? []) {
      const key = variable.contextType || "other";
      byContext.set(key, [...(byContext.get(key) ?? []), variable]);
    }
    return [...byContext.entries()];
  }, [event]);

  if (events.length === 0) {
    return <EmptyState title="No trace events" description="Nothing ran for this input." />;
  }

  const last = events.length - 1;
  return (
    <div className={classes.trace}>
      <div className={classes.traceToolbar}>
        <Button size="xs" variant="subtle" disabled={current <= 0} onClick={() => onSelect(current - 1)}>
          <Button.Icon side="left">
            <StepBack size={14} />
          </Button.Icon>
          Back
        </Button>
        <Button size="xs" variant="subtle" disabled={current >= last} onClick={() => onSelect(current + 1)}>
          <Button.Icon side="left">
            <ArrowDownToLine size={14} />
          </Button.Icon>
          Step in
        </Button>
        <Button
          size="xs"
          variant="subtle"
          disabled={current >= last}
          onClick={() => onSelect(stepOver(events, current))}
        >
          <Button.Icon side="left">
            <StepForward size={14} />
          </Button.Icon>
          Step over
        </Button>
        <Button
          size="xs"
          variant="subtle"
          disabled={current >= last}
          onClick={() => onSelect(stepOut(events, current))}
        >
          <Button.Icon side="left">
            <ArrowUpFromLine size={14} />
          </Button.Icon>
          Step out
        </Button>
        <span className={classes.spacer} />
        <span className={classes.muted}>
          {current + 1} / {events.length}
        </span>
        <Badge size="xs" variant="light" color={STATUS_COLOR[trace.status]}>
          {trace.status}
        </Badge>
      </div>
      <div className={classes.traceBody}>
        <div className={classes.traceEvents} ref={listRef}>
          {events.map((item) => (
            <button
              key={item.index}
              type="button"
              data-index={item.index}
              className={`${classes.traceEvent} ${item.index === current ? classes.traceEventActive : ""}`}
              style={{ paddingLeft: 8 + item.depth * 12 }}
              onClick={() => onSelect(item.index)}
            >
              <span className={classes.tracePhase}>{item.phase}</span>
              <span className={classes.mono}>{labelOf?.(item) ?? positionLabel(item.position)}</span>
              {item.messages.length > 0 && <CornerDownRight size={12} className={classes.muted} />}
            </button>
          ))}
        </div>
        <div className={classes.traceDetail}>
          <div className={classes.detailSection}>
            <div className={classes.detailTitle}>Stack</div>
            {stack.length === 0 ? (
              <div className={classes.muted}>—</div>
            ) : (
              [...stack].reverse().map((frame) => (
                <button
                  key={frame.index}
                  type="button"
                  className={classes.stackFrame}
                  onClick={() => onSelect(frame.index)}
                >
                  <span className={classes.mono}>{frame.position.replace(/^StructureMap\./, "")}</span>
                </button>
              ))
            )}
          </div>
          {event?.messages.length ? (
            <div className={classes.detailSection}>
              <div className={classes.detailTitle}>Log</div>
              {event.messages.map((message) => (
                <div key={message} className={classes.mono}>
                  {message}
                </div>
              ))}
            </div>
          ) : null}
          {groups.map(([context, variables]) => (
            <div key={context} className={classes.detailSection}>
              <div className={classes.detailTitle}>{context}</div>
              {variables.map((variable) => (
                <div key={variable.name} className={classes.variable}>
                  <div className={classes.variableHead}>
                    <span className={classes.mono}>{variable.name}</span>
                    {variable.dataType && <span className={classes.muted}>{variable.dataType}</span>}
                    {variable.path && <span className={`${classes.muted} ${classes.mono}`}>{variable.path}</span>}
                    {variable.path && positions?.[variable.path] && (
                      <span className={classes.muted}>line {positions[variable.path].line}</span>
                    )}
                  </div>
                  <pre className={classes.variableValue}>{valueText(variable)}</pre>
                </div>
              ))}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
