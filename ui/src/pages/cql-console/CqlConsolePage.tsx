import { Resizable } from "@octofhir/ui-kit";
import { useMutation } from "@tanstack/react-query";
import {
  Braces,
  Check,
  CircleAlert,
  Library,
  Play,
  Sigma,
  Sliders,
  Sparkles,
  SquareFunction,
  Wand2,
  X as Xmark,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { fhirClient, HttpError } from "@/shared/api/fhirClient";
import { isRecord } from "@/shared/api/guards";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import classes from "./CqlConsolePage.module.css";
import { CqlSourceEditor, type CqlSourceEditorHandle } from "./components/CqlSourceEditor";
import { FunctionPalette } from "./components/FunctionPalette";
import { ResultItem } from "./components/ResultItem";
import {
  CQL_EXAMPLES,
  DEFAULT_SOURCE,
  SAMPLE_RESOURCES,
  type SampleKey,
  sampleJsonString,
} from "./presets";
import {
  type CqlDiagnostic,
  type CqlEvaluationResult,
  parseCqlResponse,
  parseValidateResponse,
} from "./types";

const STORAGE_SRC = "octofhir.cql.source";
const STORAGE_CTX = "octofhir.cql.context";
const STORAGE_PARAMS = "octofhir.cql.params";

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
const RUN_KEY = isMac ? "⌘↵" : "Ctrl+↵";

/** Pull the real diagnostic out of an error — OperationOutcome body, else message. */
function errorMessage(err: unknown): string {
  if (err instanceof HttpError) {
    const body = err.response.data;
    if (isRecord(body) && Array.isArray(body.issue)) {
      const diagnostics = body.issue
        .map((issue) => {
          if (!isRecord(issue)) return undefined;
          const details = isRecord(issue.details) ? (issue.details.text as unknown) : undefined;
          return (issue.diagnostics ?? details) as string | undefined;
        })
        .filter((m): m is string => typeof m === "string" && m.length > 0);
      if (diagnostics.length) return diagnostics.join("; ");
    }
    if (typeof body === "string" && body.trim()) return body;
  }
  return err instanceof Error ? err.message : String(err);
}

/** A source is a library when it declares `library` or any `define`. */
function isLibrarySource(source: string): boolean {
  return /\blibrary\b/.test(source) || /\bdefine\b/.test(source);
}

type InputParam = {
  name: string;
  valueString?: string;
  valueInteger?: number;
  valueDecimal?: number;
  valueBoolean?: boolean;
  resource?: unknown;
};

function buildParamEntries(paramsJson: string): InputParam[] {
  if (!paramsJson.trim() || paramsJson.trim() === "{}") return [];
  let obj: unknown;
  try {
    obj = JSON.parse(paramsJson);
  } catch {
    throw new Error("Invalid JSON in parameters");
  }
  if (!isRecord(obj)) throw new Error("Parameters must be a JSON object");
  return Object.entries(obj).map(([name, value]) => {
    if (typeof value === "boolean") return { name, valueBoolean: value };
    if (typeof value === "number")
      return Number.isInteger(value)
        ? { name, valueInteger: value }
        : { name, valueDecimal: value };
    if (typeof value === "string") return { name, valueString: value };
    return { name, resource: value };
  });
}

export function CqlConsolePage() {
  const [source, setSource] = useState(() => localStorage.getItem(STORAGE_SRC) ?? DEFAULT_SOURCE);
  const [contextResource, setContextResource] = useState(
    () => localStorage.getItem(STORAGE_CTX) ?? ""
  );
  const [paramsJson, setParamsJson] = useState(() => localStorage.getItem(STORAGE_PARAMS) ?? "{}");

  const [showFunctions, setShowFunctions] = useState(false);
  const [inputTab, setInputTab] = useState<"context" | "params">("context");
  const [diagnostics, setDiagnostics] = useState<CqlDiagnostic[]>([]);
  const [validated, setValidated] = useState(false);
  const editorRef = useRef<CqlSourceEditorHandle>(null);

  useEffect(() => {
    localStorage.setItem(STORAGE_SRC, source);
  }, [source]);
  useEffect(() => {
    localStorage.setItem(STORAGE_CTX, contextResource);
  }, [contextResource]);
  useEffect(() => {
    localStorage.setItem(STORAGE_PARAMS, paramsJson);
  }, [paramsJson]);

  const mode = isLibrarySource(source) ? "library" : "expression";

  const contextType = useMemo(() => {
    if (!contextResource.trim()) return undefined;
    try {
      const parsed = JSON.parse(contextResource);
      return typeof parsed?.resourceType === "string" ? parsed.resourceType : undefined;
    } catch {
      return undefined;
    }
  }, [contextResource]);

  // Live parse-only validation: debounced, no evaluation/DB — surfaces syntax
  // errors as you type instead of only at Run.
  useEffect(() => {
    if (!source.trim()) {
      setDiagnostics([]);
      setValidated(false);
      return;
    }
    let cancelled = false;
    const handle = setTimeout(async () => {
      try {
        const parameter: InputParam[] = [
          { name: "validate", valueBoolean: true },
          mode === "library"
            ? { name: "library", valueString: source }
            : { name: "expression", valueString: source },
        ];
        const res = await fhirClient.customRequest({
          method: "POST",
          url: "/fhir/$cql",
          data: { resourceType: "Parameters", parameter },
          headers: { "Content-Type": "application/fhir+json" },
        });
        if (cancelled) return;
        if (isRecord(res.data)) {
          setDiagnostics(parseValidateResponse(res.data as { parameter: never[] }));
          setValidated(true);
        }
      } catch {
        // Validation is best-effort; ignore transport errors silently.
        if (!cancelled) setValidated(false);
      }
    }, 500);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [source, mode]);

  const evaluateMutation = useMutation<CqlEvaluationResult, Error>({
    mutationFn: async () => {
      const parameter: InputParam[] = [
        mode === "library"
          ? { name: "library", valueString: source }
          : { name: "expression", valueString: source },
      ];

      if (contextResource.trim()) {
        let resource: unknown;
        try {
          resource = JSON.parse(contextResource);
        } catch {
          throw new Error("Invalid JSON in context resource");
        }
        if (contextType) parameter.push({ name: "context", valueString: contextType });
        parameter.push({ name: "contextValue", resource });
      }

      parameter.push(...buildParamEntries(paramsJson));

      let response: Awaited<ReturnType<typeof fhirClient.customRequest>>;
      try {
        response = await fhirClient.customRequest({
          method: "POST",
          url: "/fhir/$cql",
          data: { resourceType: "Parameters", parameter },
          headers: { "Content-Type": "application/fhir+json" },
        });
      } catch (err) {
        throw new Error(errorMessage(err));
      }

      const data = response.data;
      if (!isRecord(data) || !Array.isArray((data as { parameter?: unknown }).parameter)) {
        throw new Error("Invalid CQL response");
      }
      return parseCqlResponse(data as { parameter: never[] });
    },
  });

  const handleExecute = () => evaluateMutation.mutate();

  const handleClear = () => {
    setSource("");
    evaluateMutation.reset();
  };

  const loadExample = (ex: (typeof CQL_EXAMPLES)[number]) => {
    setSource(ex.source);
    if (ex.sample) setContextResource(sampleJsonString(ex.sample));
  };

  const loadSample = (key: SampleKey) => {
    setContextResource(sampleJsonString(key));
    setInputTab("context");
  };

  const formatResource = () => {
    try {
      setContextResource(JSON.stringify(JSON.parse(contextResource), null, 2));
    } catch {
      /* leave invalid JSON untouched */
    }
  };

  const activeSampleKey = SAMPLE_RESOURCES.find(
    (s) => sampleJsonString(s.key) === contextResource
  )?.key;

  const insertFunction = (snippet: string) => {
    const handle = editorRef.current;
    if (handle) {
      handle.insertSnippet(snippet);
    } else {
      setSource((prev) => prev + snippet.replace(/\$\{\d+:?([^}]*)\}/g, "$1"));
    }
  };

  const data = evaluateMutation.data;

  return (
    <ToolWorkspaceLayout
      title="CQL Console"
      description="Evaluate CQL expressions and libraries — every define, against an optional resource context"
      className="page-enter"
      actions={
        <div className={classes.actions}>
          <button
            type="button"
            className={classes.runBtn}
            onClick={handleExecute}
            disabled={evaluateMutation.isPending || !source.trim()}
          >
            <Play size={15} />
            {evaluateMutation.isPending ? "Running…" : "Run"}
            <span className={classes.kbd}>{RUN_KEY}</span>
          </button>
          <button type="button" className={classes.ghostBtn} onClick={handleClear}>
            <Xmark size={15} />
            Clear
          </button>
        </div>
      }
    >
      <div className={classes.workspaceResizable}>
        <Resizable.Group orientation="vertical">
          {/* ── CQL source ── */}
          <Resizable.Pane defaultSize={38} minSize={18}>
            <div className={classes.editorPanel}>
              <div className={classes.panelHead}>
                <span className={classes.panelTitle}>
                  <span className={classes.panelTitleIcon}>
                    {mode === "library" ? <Library size={14} /> : <SquareFunction size={14} />}
                  </span>
                  CQL Source
                </span>
                <span
                  className={classes.modeBadge}
                  title={
                    mode === "library" ? "Library — every define is evaluated" : "Single expression"
                  }
                >
                  {mode}
                </span>
                {contextType && (
                  <span className={classes.ctxBadge} title="CQL evaluation context">
                    context {contextType}
                  </span>
                )}
                <span className={classes.panelHeadSpacer} />
                <div className={classes.chipRow}>
                  <span className={classes.chipRowLabel}>
                    <Sparkles size={12} style={{ verticalAlign: "-2px", marginRight: 4 }} />
                    Examples
                  </span>
                  {CQL_EXAMPLES.map((ex) => (
                    <button
                      key={ex.label}
                      type="button"
                      className={classes.chip}
                      onClick={() => loadExample(ex)}
                    >
                      {ex.label}
                    </button>
                  ))}
                  <button
                    type="button"
                    className={`${classes.chip} ${showFunctions ? classes.chipActive : ""}`}
                    onClick={() => setShowFunctions((v) => !v)}
                    title="Browse CQL functions"
                  >
                    <Sigma size={12} />
                    Functions
                  </button>
                </div>
              </div>
              {showFunctions && <FunctionPalette onInsert={(fn) => insertFunction(fn.snippet)} />}
              <div className={classes.editorHost}>
                <CqlSourceEditor
                  ref={editorRef}
                  value={source}
                  onChange={setSource}
                  onSubmit={handleExecute}
                  height="100%"
                />
              </div>
              {source.trim() &&
                (diagnostics.length > 0 ? (
                  <div className={`${classes.diagBar} ${classes.diagBarError}`}>
                    {diagnostics.map((d) => (
                      <div key={d.message} className={classes.diagItem}>
                        <CircleAlert size={13} />
                        <span>{d.message}</span>
                      </div>
                    ))}
                  </div>
                ) : (
                  validated && (
                    <div className={`${classes.diagBar} ${classes.diagBarOk}`}>
                      <Check size={13} />
                      Valid CQL
                    </div>
                  )
                ))}
            </div>
          </Resizable.Pane>

          <Resizable.Handle />

          {/* ── Input: Context / Parameters ── */}
          <Resizable.Pane defaultSize={28} minSize={12}>
            <div className={classes.editorPanel}>
              <div className={classes.panelHead}>
                <div className={classes.tabBar}>
                  <button
                    type="button"
                    className={`${classes.tab} ${inputTab === "context" ? classes.tabActive : ""}`}
                    onClick={() => setInputTab("context")}
                  >
                    <Braces size={13} />
                    Context
                  </button>
                  <button
                    type="button"
                    className={`${classes.tab} ${inputTab === "params" ? classes.tabActive : ""}`}
                    onClick={() => setInputTab("params")}
                  >
                    <Sliders size={13} />
                    Parameters
                  </button>
                </div>
                <span className={classes.panelHeadSpacer} />
                {inputTab === "context" && (
                  <div className={classes.chipRow}>
                    {SAMPLE_RESOURCES.map((s) => (
                      <button
                        key={s.key}
                        type="button"
                        className={`${classes.chip} ${activeSampleKey === s.key ? classes.chipActive : ""}`}
                        onClick={() => loadSample(s.key)}
                      >
                        {s.label}
                      </button>
                    ))}
                    <button
                      type="button"
                      className={classes.chip}
                      onClick={() => setContextResource("")}
                      title="Clear context"
                    >
                      None
                    </button>
                    <button
                      type="button"
                      className={classes.chip}
                      onClick={formatResource}
                      title="Format JSON"
                    >
                      <Wand2 size={12} />
                      Format
                    </button>
                  </div>
                )}
              </div>
              <div className={classes.editorHost}>
                {inputTab === "context" ? (
                  <JsonEditor
                    value={contextResource}
                    onChange={setContextResource}
                    onExecute={handleExecute}
                    height="100%"
                  />
                ) : (
                  <JsonEditor
                    value={paramsJson}
                    onChange={setParamsJson}
                    onExecute={handleExecute}
                    height="100%"
                  />
                )}
              </div>
            </div>
          </Resizable.Pane>

          <Resizable.Handle />

          {/* ── Results ── */}
          <Resizable.Pane defaultSize={34} minSize={14}>
            <div className={classes.resultsPanel}>
              {evaluateMutation.error && (
                <div className={classes.errorBox}>
                  <CircleAlert size={18} className={classes.errorIcon} />
                  <div>
                    <div className={classes.errorTitle}>Evaluation Error</div>
                    <div className={classes.errorMsg}>{evaluateMutation.error.message}</div>
                  </div>
                </div>
              )}

              {data ? (
                <div className={classes.resultSection}>
                  <div className={classes.resultSectionHead}>
                    <span className={classes.sectionTitle}>
                      {data.mode === "library" ? "Defines" : "Result"}
                    </span>
                    <span className={classes.sectionCount}>{data.defines.length}</span>
                  </div>
                  {data.defines.length === 0 ? (
                    <div className={classes.emptyHint}>
                      No public defines were returned. Mark defines as <code>public</code> or check
                      the source.
                    </div>
                  ) : (
                    <div className={classes.resultList}>
                      {data.defines.map((def) => (
                        <ResultItem
                          key={def.name}
                          define={def}
                          hideName={data.mode === "expression"}
                        />
                      ))}
                    </div>
                  )}
                </div>
              ) : (
                !evaluateMutation.error && (
                  <div className={classes.emptyState}>
                    <span className={classes.emptyIcon}>
                      <Library size={24} />
                    </span>
                    <span className={classes.emptyTitle}>Ready to evaluate</span>
                    <span className={classes.emptyHint}>
                      Write a CQL expression or a full library with multiple <code>define</code>s,
                      then press <code>{RUN_KEY}</code>. Every public define is evaluated.
                    </span>
                  </div>
                )
              )}
            </div>
          </Resizable.Pane>
        </Resizable.Group>
      </div>
    </ToolWorkspaceLayout>
  );
}
