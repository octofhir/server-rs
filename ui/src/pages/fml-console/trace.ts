/**
 * The debug trace of `$transform?debug=true`, as the Mapping Language IG's debug page defines
 * it: `trace` events with phase, depth, position and variables; `traceStatus`; `outcome`.
 * Plus the replay rules of that page: the stack is the open `enter` events, stepping over skips
 * deeper events, stepping out moves to a shallower one.
 */

import { isRecord } from "@/shared/api/guards";

export type Phase = "enter" | "step" | "exit";

export interface TraceVariable {
  name: string;
  contextType: string;
  dataType?: string;
  /** Where the value is in the input (`Patient.name[0]`), for values that come from it. */
  path?: string;
  /** The value as JSON (resources, data types and backbone elements alike), or undefined for an empty collection. */
  value?: unknown;
}

export interface TraceEvent {
  index: number;
  phase: Phase;
  depth: number;
  source?: string;
  position: string;
  /** 1-based editor coordinates of the event's text, when the map was given as FML. */
  range?: { startLine: number; startColumn: number; endLine: number; endColumn: number };
  variables: TraceVariable[];
  messages: string[];
}

export interface OutcomeIssue {
  severity: string;
  code: string;
  diagnostics?: string;
  expression?: string[];
  line?: number;
  column?: number;
}

export interface DebugResult {
  status: "complete" | "truncated" | "stopped";
  events: TraceEvent[];
  result?: unknown;
  issues: OutcomeIssue[];
}

type Part = Record<string, unknown> & { name?: string; part?: Part[] };

function partValue(part: Part): unknown {
  for (const [key, value] of Object.entries(part)) {
    if (key.startsWith("value")) return value;
  }
  return undefined;
}

function variableOf(parts: Part[]): TraceVariable {
  const variable: TraceVariable = { name: "", contextType: "" };
  for (const part of parts) {
    switch (part.name) {
      case "name":
        variable.name = String(partValue(part) ?? "");
        break;
      case "contextType":
        variable.contextType = String(partValue(part) ?? "");
        break;
      case "dataType":
        variable.dataType = String(partValue(part) ?? "");
        break;
      case "path":
        variable.path = String(partValue(part) ?? "");
        break;
      case "resource":
        variable.value = part.resource;
        break;
      case "jsonValue":
        try {
          variable.value = JSON.parse(String(partValue(part)));
        } catch {
          variable.value = partValue(part);
        }
        break;
      case "value":
        variable.value = partValue(part);
        break;
    }
  }
  return variable;
}

export function issuesOf(outcome: unknown): OutcomeIssue[] {
  if (!isRecord(outcome) || !Array.isArray(outcome.issue)) return [];
  return outcome.issue.filter(isRecord).map((issue) => {
    const extensions = Array.isArray(issue.extension) ? issue.extension.filter(isRecord) : [];
    const ext = (suffix: string) =>
      extensions.find((e) => String(e.url).endsWith(suffix))?.valueInteger as number | undefined;
    return {
      severity: String(issue.severity ?? "error"),
      code: String(issue.code ?? ""),
      diagnostics: typeof issue.diagnostics === "string" ? issue.diagnostics : undefined,
      expression: Array.isArray(issue.expression) ? issue.expression.map(String) : undefined,
      line: ext("issue-line"),
      column: ext("issue-col"),
    };
  });
}

/** Reads the Parameters a `$transform?debug=true` returns. */
export function parseDebugParameters(parameters: unknown): DebugResult {
  const result: DebugResult = { status: "complete", events: [], issues: [] };
  if (!isRecord(parameters) || !Array.isArray(parameters.parameter)) return result;
  for (const parameter of parameters.parameter as Part[]) {
    switch (parameter.name) {
      case "return":
        result.result = parameter.resource;
        break;
      case "traceStatus":
        result.status = String(partValue(parameter)) as DebugResult["status"];
        break;
      case "outcome":
        result.issues = issuesOf(parameter.resource);
        break;
      case "trace": {
        const parts = parameter.part ?? [];
        const event: TraceEvent = {
          index: result.events.length,
          phase: "step",
          depth: 0,
          position: "",
          variables: [],
          messages: [],
        };
        const coordinate: Record<string, number> = {};
        for (const part of parts) {
          switch (part.name) {
            case "phase":
              event.phase = String(partValue(part)) as Phase;
              break;
            case "depth":
              event.depth = Number(partValue(part) ?? 0);
              break;
            case "source":
              event.source = String(partValue(part));
              break;
            case "expressionPosition":
              event.position = String(partValue(part));
              break;
            case "startRow":
            case "startColumn":
            case "endRow":
            case "endColumn":
              coordinate[part.name] = Number(partValue(part));
              break;
            case "variable":
              event.variables.push(variableOf(part.part ?? []));
              break;
            case "message":
              for (const text of part.part ?? []) {
                if (text.name === "text") event.messages.push(String(partValue(text)));
              }
              break;
          }
        }
        if (coordinate.startRow !== undefined && coordinate.startColumn !== undefined) {
          // The trace counts from 0; the editor from 1.
          event.range = {
            startLine: coordinate.startRow + 1,
            startColumn: coordinate.startColumn + 1,
            endLine: (coordinate.endRow ?? coordinate.startRow) + 1,
            endColumn: (coordinate.endColumn ?? coordinate.startColumn) + 1,
          };
        }
        result.events.push(event);
        break;
      }
    }
  }
  return result;
}

/** The open scopes at `index`, outermost first: every `enter` up to it not yet closed before
 * it (at its own `exit` a scope is still open). */
export function stackAt(events: TraceEvent[], index: number): TraceEvent[] {
  const stack: TraceEvent[] = [];
  for (let i = 0; i < index && i < events.length; i++) {
    const event = events[i];
    if (event.phase === "enter") stack.push(event);
    else if (event.phase === "exit") stack.pop();
  }
  if (events[index]?.phase === "enter") stack.push(events[index]);
  return stack;
}

/** The input location the event is about: the deepest path among its variables. */
export function inputPathAt(event: TraceEvent | undefined): string | undefined {
  return event?.variables
    .map((variable) => variable.path)
    .filter((path): path is string => !!path)
    .reduce<string | undefined>(
      (deepest, path) => (!deepest || path.length > deepest.length ? path : deepest),
      undefined
    );
}

/** Step over: the next event no deeper than the current one. */
export function stepOver(events: TraceEvent[], index: number): number {
  const depth = events[index]?.depth ?? 0;
  for (let i = index + 1; i < events.length; i++) {
    if (events[i].depth <= depth) return i;
  }
  return events.length - 1;
}

/** Step out: the next event shallower than the current one. */
export function stepOut(events: TraceEvent[], index: number): number {
  const depth = events[index]?.depth ?? 0;
  for (let i = index + 1; i < events.length; i++) {
    if (events[i].depth < depth) return i;
  }
  return events.length - 1;
}

/** A short label for a position: `group[0].rule[2].target[1]` → `rule 2 · target 1`. */
export function positionLabel(position: string): string {
  const parts = position.replace(/^StructureMap\./, "").split(".");
  const last = parts.at(-1) ?? position;
  const match = /^(\w+)\[(\d+)\]$/.exec(last);
  return match ? `${match[1]} ${match[2]}` : last;
}
