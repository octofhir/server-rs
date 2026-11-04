/**
 * FML console: write a map (or open a stored one), check it as you type, run it over a resource
 * with `StructureMap/$transform` (with models of its own: logical models and ConceptMaps) or over
 * a CDA document (`/api/fml/cda-run`: the CDA model, the C-CDA templates and the converter's
 * maps), replay the debug trace against the source and the input, save the map as a
 * StructureMap, and share all of it as a link.
 */

import {
  Badge,
  Button,
  notifications,
  Resizable,
  SegmentedRadioGroup,
  Switch,
  Tabs,
  TextInput,
} from "@octofhir/ui-kit";
import { useMutation } from "@tanstack/react-query";
import { CircleAlert, Download, FileUp, Link2, Play, Save, TriangleAlert, Wand2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fhirClient, HttpError } from "@/shared/api/fhirClient";
import { isRecord } from "@/shared/api/guards";
import { usePaletteContribution } from "@/shared/command-palette";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import classes from "./FmlConsolePage.module.css";
import { FmlSourceEditor } from "./components/FmlSourceEditor";
import { MapPicker, openStoredMap } from "./components/MapPicker";
import { TracePanel } from "./components/TracePanel";
import { DEFAULT_SOURCE, SAMPLE_INPUTS, type SampleKey, sampleJson } from "./presets";
import { locate } from "./jsonLocate";
import {
  type DebugResult,
  inputPathAt,
  issuesOf,
  type OutcomeIssue,
  parseDebugParameters,
} from "./trace";
import { shareLink, sharedState } from "./share";
import { type CheckResult, parseCheckResult } from "./types";

const STORAGE_SOURCE = "octofhir.fml.source";
const STORAGE_INPUT = "octofhir.fml.input";
const STORAGE_TRACE = "octofhir.fml.trace";
const STORAGE_MODELS = "octofhir.fml.models";
const STORAGE_CDA = "octofhir.fml.cda";

/** StructureDefinitions of logical models and ConceptMaps for this run only: an array or a Bundle. */
const MODELS_HINT = "[]";

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
const RUN_KEY = isMac ? "⌘↵" : "Ctrl+↵";

function stored(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

function store(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* storage unavailable */
  }
}

interface RunResult {
  result?: unknown;
  trace?: DebugResult;
  issues: OutcomeIssue[];
  /** For a CDA input: where each input path is in the XML. */
  positions?: Record<string, { line: number; column: number }>;
  /** The run asked for a trace and was refused one (the role is missing). */
  traceRefused: boolean;
}

/** The OperationOutcome issues of a failed request, or its message. */
function failureIssues(error: unknown): OutcomeIssue[] {
  if (error instanceof HttpError) {
    const issues = issuesOf(error.response.data);
    if (issues.length) return issues;
  }
  return [
    {
      severity: "error",
      code: "exception",
      diagnostics: error instanceof Error ? error.message : String(error),
    },
  ];
}

/** The `model` parameters: each resource of an array, or the one resource (a Bundle, say). */
function modelParameters(models: string): { name: string; resource: unknown }[] {
  const text = models.trim();
  if (!text || text === MODELS_HINT) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new Error("The models are not valid JSON");
  }
  const resources = Array.isArray(parsed) ? parsed : [parsed];
  return resources.map((resource) => ({ name: "model", resource }));
}

async function transform(
  source: string,
  input: unknown,
  models: string,
  debug: boolean
): Promise<RunResult> {
  const response = await fhirClient.customRequest({
    method: "POST",
    url: `/fhir/StructureMap/$transform${debug ? "?debug=true" : ""}`,
    data: {
      resourceType: "Parameters",
      parameter: [
        { name: "srcMap", valueString: source },
        { name: "content", resource: input },
        ...modelParameters(models),
      ],
    },
    headers: { "Content-Type": "application/fhir+json" },
  });
  if (debug) {
    const trace = parseDebugParameters(response.data);
    return { result: trace.result, trace, issues: trace.issues, traceRefused: false };
  }
  return { result: response.data, issues: [], traceRefused: false };
}

/** A map run over a CDA document: read into the CDA model, run with the converter's maps. */
async function runOnCda(source: string, xml: string, debug: boolean): Promise<RunResult> {
  const response = await fhirClient.customRequest({
    method: "POST",
    url: "/api/fml/cda-run",
    data: { source, xml, debug },
    headers: { "Content-Type": "application/json" },
  });
  const data = isRecord(response.data) ? response.data : {};
  const trace = data.debug ? parseDebugParameters(data.debug) : undefined;
  const readIssues: OutcomeIssue[] = Array.isArray(data.issues)
    ? data.issues.filter(isRecord).map((issue) => ({
        severity: "warning",
        code: "not-supported",
        diagnostics: String(issue.message ?? ""),
        expression: typeof issue.path === "string" ? [issue.path] : undefined,
        line: typeof issue.line === "number" ? issue.line : undefined,
      }))
    : [];
  const failure: OutcomeIssue[] =
    typeof data.error === "string" ? [{ severity: "error", code: "processing", diagnostics: data.error }] : [];
  const positions = isRecord(data.positions)
    ? (data.positions as Record<string, { line: number; column: number }>)
    : undefined;
  return {
    result: data.result ?? undefined,
    trace,
    issues: [...failure, ...(trace?.issues ?? []), ...readIssues],
    positions,
    traceRefused: false,
  };
}

/** Character offsets of a line of a text (1-based), for highlighting it. */
function lineRange(text: string, line: number): { start: number; end: number } | undefined {
  let start = 0;
  for (let n = 1; n < line; n++) {
    const next = text.indexOf("\n", start);
    if (next < 0) return undefined;
    start = next + 1;
  }
  const end = text.indexOf("\n", start);
  return { start, end: end < 0 ? text.length : end };
}

export function FmlConsolePage() {
  const [source, setSource] = useState(() => stored(STORAGE_SOURCE, DEFAULT_SOURCE));
  const [input, setInput] = useState(() => stored(STORAGE_INPUT, sampleJson("patient")));
  const [traceOn, setTraceOn] = useState(() => stored(STORAGE_TRACE, "true") === "true");
  const [models, setModels] = useState(() => stored(STORAGE_MODELS, MODELS_HINT));
  const [cda, setCda] = useState(() => stored(STORAGE_CDA, ""));
  const [inputTab, setInputTab] = useState("input");
  /** What a run takes: the JSON input or the CDA document, the last of the two shown. */
  const [runOn, setRunOn] = useState<"json" | "cda">("json");
  const fileRef = useRef<HTMLInputElement>(null);
  const [reference, setReference] = useState("");
  const [check, setCheck] = useState<CheckResult>({ problems: [] });
  const [tab, setTab] = useState("result");
  const [current, setCurrent] = useState(0);

  useEffect(() => store(STORAGE_SOURCE, source), [source]);
  useEffect(() => store(STORAGE_INPUT, input), [input]);
  useEffect(() => store(STORAGE_TRACE, String(traceOn)), [traceOn]);
  useEffect(() => store(STORAGE_MODELS, models), [models]);
  useEffect(() => store(STORAGE_CDA, cda), [cda]);
  useEffect(() => {
    if (inputTab === "input") setRunOn("json");
    if (inputTab === "cda") setRunOn("cda");
  }, [inputTab]);

  // A shared link opens with the map, input and models it carries.
  useEffect(() => {
    sharedState(window.location.hash).then((state) => {
      if (!state) return;
      setSource(state.source);
      setInput(state.input);
      if (state.models) setModels(state.models);
      history.replaceState(null, "", window.location.pathname + window.location.search);
    });
  }, []);

  // Check as you type: syntax, compile and type problems from the server.
  useEffect(() => {
    let cancelled = false;
    const handle = setTimeout(async () => {
      try {
        const response = await fhirClient.customRequest({
          method: "POST",
          url: "/api/fml/check",
          data: { source },
          headers: { "Content-Type": "application/json" },
        });
        if (!cancelled) setCheck(parseCheckResult(response.data));
      } catch {
        // Checking is best effort; the run reports what matters.
      }
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [source]);

  const run = useMutation<RunResult, Error>({
    mutationFn: async () => {
      if (runOn === "cda") {
        if (!cda.trim()) throw new Error("Load a CDA document first");
        try {
          return await runOnCda(source, cda, traceOn);
        } catch (error) {
          return { issues: failureIssues(error), traceRefused: false };
        }
      }
      let content: unknown;
      try {
        content = JSON.parse(input);
      } catch {
        throw new Error("The input is not valid JSON");
      }
      try {
        return await transform(source, content, models, traceOn);
      } catch (error) {
        // Without the role for traces, run without one and say so.
        if (traceOn && error instanceof HttpError && error.response.status === 403) {
          try {
            return { ...(await transform(source, content, models, false)), traceRefused: true };
          } catch (retry) {
            return { issues: failureIssues(retry), traceRefused: true };
          }
        }
        return { issues: failureIssues(error), traceRefused: false };
      }
    },
    onSuccess: (data) => {
      setCurrent(Math.max(0, (data.trace?.events.length ?? 1) - 1));
      setTab(data.result === undefined ? (data.trace ? "trace" : "issues") : "result");
    },
  });

  const format = useMutation<void, Error>({
    mutationFn: async () => {
      const response = await fhirClient.customRequest({
        method: "POST",
        url: "/api/fml/format",
        data: { source },
        headers: { "Content-Type": "application/json" },
      });
      if (isRecord(response.data) && typeof response.data.source === "string") {
        setSource(response.data.source);
      }
    },
    onError: () =>
      notifications.show({ color: "red", title: "Not formatted", message: "Fix the syntax errors first." }),
  });

  const save = useMutation<string, Error>({
    mutationFn: async () => {
      let compiled: Awaited<ReturnType<typeof fhirClient.customRequest>>;
      try {
        compiled = await fhirClient.customRequest({
          method: "POST",
          url: "/api/fml/compile",
          data: { source },
          headers: { "Content-Type": "application/json" },
        });
      } catch {
        throw new Error("The map does not compile");
      }
      const map = compiled.data;
      if (!isRecord(map) || typeof map.id !== "string") {
        throw new Error("Give the map a name (/// name = '…') to save it");
      }
      await fhirClient.customRequest({
        method: "PUT",
        url: `/fhir/StructureMap/${map.id}`,
        data: map,
        headers: { "Content-Type": "application/fhir+json" },
      });
      return map.id;
    },
    onSuccess: (id) =>
      notifications.show({ color: "green", title: "Saved", message: `StructureMap/${id}` }),
    onError: (error) =>
      notifications.show({ color: "red", title: "Not saved", message: error.message }),
  });

  const share = useMutation<string, Error>({
    mutationFn: async () => {
      const link = await shareLink({ source, input, models });
      await navigator.clipboard.writeText(link);
      return link;
    },
    onSuccess: () =>
      notifications.show({ color: "green", title: "Link copied", message: "The map, input and models are in the link." }),
    onError: (error) => notifications.show({ color: "red", title: "Not shared", message: error.message }),
  });

  const open = useMutation<string, Error, string>({
    mutationFn: openStoredMap,
    onSuccess: (text) => setSource(text),
    onError: (error) => notifications.show({ color: "red", title: "Not opened", message: error.message }),
  });

  const load = useMutation<string, Error>({
    mutationFn: async () => {
      const path = reference.trim().replace(/^\/+/, "");
      if (!/^[A-Za-z]+\/[A-Za-z0-9\-.]{1,64}$/.test(path)) {
        throw new Error("Give a reference such as Patient/123");
      }
      const response = await fhirClient.customRequest({ method: "GET", url: `/fhir/${path}` });
      return JSON.stringify(response.data, null, 2);
    },
    onSuccess: (json) => setInput(json),
    onError: (error) =>
      notifications.show({
        color: "red",
        title: "Not loaded",
        message: error instanceof HttpError ? `${error.response.status}: ${reference}` : error.message,
      }),
  });

  const runMap = useCallback(() => run.mutate(), [run]);

  usePaletteContribution(
    "fml-console",
    useMemo(
      () => [
        { id: "fml-run", label: "Run the map", group: "FML", icon: Play, onSelect: () => run.mutate() },
        { id: "fml-format", label: "Format the map", group: "FML", icon: Wand2, onSelect: () => format.mutate() },
        { id: "fml-save", label: "Save as StructureMap", group: "FML", icon: Save, onSelect: () => save.mutate() },
        { id: "fml-share", label: "Copy a link to this map", group: "FML", icon: Link2, onSelect: () => share.mutate() },
      ],
      [run, format, save, share]
    )
  );

  const data = run.data;
  const trace = data?.trace;
  const event = trace?.events[current];
  const errors = check.problems.filter((p) => p.severity === "error").length;
  const warnings = check.problems.length - errors;
  const issues = data?.issues ?? [];
  // While replaying, the input value the current event is about.
  const inputPath = tab === "trace" ? inputPathAt(event) : undefined;
  const inputHighlight = useMemo(
    () => (inputPath ? locate(input, inputPath) : undefined),
    [input, inputPath]
  );
  // In a CDA document: the line of the element the event is about.
  const cdaLine = inputPath ? data?.positions?.[inputPath]?.line : undefined;
  const cdaHighlight = useMemo(
    () => (cdaLine ? lineRange(cda, cdaLine) : undefined),
    [cda, cdaLine]
  );

  const loadCdaFile = (file: File | undefined) => {
    if (!file) return;
    file
      .text()
      .then((text) => {
        setCda(text);
        setInputTab("cda");
      })
      .catch(() => notifications.show({ color: "red", title: "Not loaded", message: file.name }));
  };

  const loadSample = (key: SampleKey) => setInput(sampleJson(key));

  return (
    <ToolWorkspaceLayout
      title="FML"
      description="Write a FHIR Mapping Language map, run it over a resource or a CDA document, step through what it did"
      className="page-enter"
      actions={
        <div className={classes.actions}>
          <MapPicker onOpen={(id) => open.mutate(id)} />
          <Switch size="sm" checked={traceOn} onChange={setTraceOn} content="Trace" />
          <Button size="sm" variant="subtle" loading={format.isPending} onClick={() => format.mutate()}>
            <Button.Icon side="left">
              <Wand2 size={14} />
            </Button.Icon>
            Format
          </Button>
          <Button size="sm" variant="subtle" loading={share.isPending} onClick={() => share.mutate()}>
            <Button.Icon side="left">
              <Link2 size={14} />
            </Button.Icon>
            Share
          </Button>
          <Button size="sm" variant="outline" loading={save.isPending} onClick={() => save.mutate()}>
            <Button.Icon side="left">
              <Save size={14} />
            </Button.Icon>
            Save
          </Button>
          <Button
            size="sm"
            variant="filled"
            color="primary"
            loading={run.isPending}
            disabled={!source.trim()}
            onClick={runMap}
          >
            <Button.Icon side="left">
              <Play size={14} />
            </Button.Icon>
            Run
            <span className={classes.kbd}>{RUN_KEY}</span>
          </Button>
        </div>
      }
    >
      <div className={classes.workspace}>
        <Resizable.Group orientation="horizontal">
          <Resizable.Pane defaultSize={55} minSize={30}>
            <div className={classes.panel}>
              <div className={classes.panelHead}>
                <span className={classes.panelTitle}>Map</span>
                {check.url && <span className={classes.mono}>{check.url}</span>}
                <span className={classes.spacer} />
                {errors > 0 && (
                  <Badge size="xs" variant="light" color="red">
                    {errors} error{errors === 1 ? "" : "s"}
                  </Badge>
                )}
                {warnings > 0 && (
                  <Badge size="xs" variant="light" color="orange">
                    {warnings} warning{warnings === 1 ? "" : "s"}
                  </Badge>
                )}
              </div>
              <div className={classes.editorHost}>
                <FmlSourceEditor
                  value={source}
                  onChange={setSource}
                  onSubmit={runMap}
                  problems={check.problems}
                  highlight={tab === "trace" ? event?.range : undefined}
                />
              </div>
            </div>
          </Resizable.Pane>
          <Resizable.Handle />
          <Resizable.Pane defaultSize={45} minSize={25}>
            <Resizable.Group orientation="vertical">
              <Resizable.Pane defaultSize={35} minSize={15}>
                <div className={classes.panel}>
                  <div className={classes.panelHead}>
                    <SegmentedRadioGroup
                      size="sm"
                      value={inputTab}
                      onChange={setInputTab}
                      options={[
                        { value: "input", content: "Input" },
                        { value: "cda", content: "CDA" },
                        { value: "models", content: "Models" },
                      ]}
                    />
                    <span className={classes.spacer} />
                    {inputTab === "input" && (
                      <>
                        <TextInput
                          size="sm"
                          placeholder="Patient/123"
                          value={reference}
                          onChange={setReference}
                          onKeyDown={(event) => {
                            if (event.key === "Enter") load.mutate();
                          }}
                        />
                        <Button size="xs" variant="subtle" loading={load.isPending} onClick={() => load.mutate()}>
                          <Button.Icon side="left">
                            <Download size={13} />
                          </Button.Icon>
                          Load
                        </Button>
                        {(Object.keys(SAMPLE_INPUTS) as SampleKey[]).map((key) => (
                          <Button key={key} size="xs" variant="subtle" onClick={() => loadSample(key)}>
                            {key}
                          </Button>
                        ))}
                      </>
                    )}
                    {inputTab === "cda" && (
                      <>
                        <input
                          ref={fileRef}
                          type="file"
                          accept=".xml,text/xml,application/xml"
                          hidden
                          onChange={(event) => {
                            loadCdaFile(event.target.files?.[0]);
                            event.target.value = "";
                          }}
                        />
                        <Button size="xs" variant="subtle" onClick={() => fileRef.current?.click()}>
                          <Button.Icon side="left">
                            <FileUp size={13} />
                          </Button.Icon>
                          Load CDA file
                        </Button>
                      </>
                    )}
                  </div>
                  <div className={classes.editorHost}>
                    {inputTab === "input" ? (
                      <JsonEditor
                        value={input}
                        onChange={setInput}
                        onExecute={runMap}
                        height="100%"
                        highlight={inputHighlight}
                        highlightClassName={classes.traceHighlight}
                      />
                    ) : inputTab === "cda" ? (
                      <JsonEditor
                        language="xml"
                        value={cda}
                        onChange={setCda}
                        onExecute={runMap}
                        height="100%"
                        highlight={runOn === "cda" ? cdaHighlight : undefined}
                        highlightClassName={classes.traceHighlight}
                      />
                    ) : (
                      <JsonEditor value={models} onChange={setModels} onExecute={runMap} height="100%" />
                    )}
                  </div>
                </div>
              </Resizable.Pane>
              <Resizable.Handle />
              <Resizable.Pane defaultSize={65} minSize={20}>
                <div className={classes.panel}>
                  <Tabs value={tab} onChange={(value) => setTab(value ?? "result")} size="sm" className={classes.tabs}>
                    <Tabs.List>
                      <Tabs.Tab value="result">Result</Tabs.Tab>
                      <Tabs.Tab value="trace" disabled={!trace}>
                        Trace{trace ? ` · ${trace.events.length}` : ""}
                      </Tabs.Tab>
                      <Tabs.Tab value="issues">Issues{issues.length ? ` · ${issues.length}` : ""}</Tabs.Tab>
                    </Tabs.List>
                  </Tabs>
                  <div className={classes.tabBody}>
                    {tab === "result" &&
                      (data?.result !== undefined ? (
                        <JsonEditor value={JSON.stringify(data.result, null, 2)} readOnly height="100%" />
                      ) : (
                        <div className={classes.placeholder}>
                          {run.isPending ? "Running…" : `Run the map (${RUN_KEY}) to see its result here.`}
                        </div>
                      ))}
                    {tab === "trace" && trace && (
                      <TracePanel trace={trace} current={current} onSelect={setCurrent} positions={data?.positions} />
                    )}
                    {tab === "issues" && (
                      <div className={classes.issues}>
                        {data?.traceRefused && (
                          <div className={classes.issue}>
                            <TriangleAlert size={14} className={classes.warning} />
                            <span>Ran without a trace: traces need the debug role.</span>
                          </div>
                        )}
                        {issues.length === 0 && !data?.traceRefused ? (
                          <div className={classes.placeholder}>No issues.</div>
                        ) : (
                          issues.map((issue) => (
                            <div key={`${issue.code}-${issue.diagnostics}`} className={classes.issue}>
                              {issue.severity === "warning" ? (
                                <TriangleAlert size={14} className={classes.warning} />
                              ) : (
                                <CircleAlert size={14} className={classes.error} />
                              )}
                              <span>
                                {issue.diagnostics}
                                {issue.line !== undefined && (
                                  <span className={classes.muted}> — line {issue.line}</span>
                                )}
                                {issue.expression?.[0] && (
                                  <span className={`${classes.muted} ${classes.mono}`}>
                                    {" "}
                                    {issue.expression[0]}
                                  </span>
                                )}
                              </span>
                            </div>
                          ))
                        )}
                      </div>
                    )}
                  </div>
                </div>
              </Resizable.Pane>
            </Resizable.Group>
          </Resizable.Pane>
        </Resizable.Group>
      </div>
    </ToolWorkspaceLayout>
  );
}
