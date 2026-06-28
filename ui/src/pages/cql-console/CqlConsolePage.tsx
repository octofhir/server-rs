import { Resizable } from "@octofhir/ui-kit";
import { useMutation } from "@tanstack/react-query";
import {
  Braces,
  CircleAlert,
  Play,
  Sigma,
  Sparkles,
  SquareFunction,
  Wand2,
  X as Xmark,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { assertFhirResource } from "@/shared/api/guards";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import classes from "./CqlConsolePage.module.css";
import {
  CqlExpressionEditor,
  type CqlExpressionEditorHandle,
} from "./components/CqlExpressionEditor";
import { FunctionPalette } from "./components/FunctionPalette";
import { ResultItem } from "./components/ResultItem";
import { EXPRESSION_EXAMPLES, SAMPLE_RESOURCES, type SampleKey, sampleJsonString } from "./presets";
import { type CqlEvaluationResult, parseCqlResponse } from "./types";

const STORAGE_EXPR = "octofhir.cql.expression";
const STORAGE_RESOURCE = "octofhir.cql.resource";

const DEFAULT_EXPRESSION = "1 + 1";
const DEFAULT_RESOURCE = "";

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
const RUN_KEY = isMac ? "⌘↵" : "Ctrl+↵";

export function CqlConsolePage() {
  const [expression, setExpression] = useState(
    () => localStorage.getItem(STORAGE_EXPR) ?? DEFAULT_EXPRESSION
  );
  const [contextResource, setContextResource] = useState(
    () => localStorage.getItem(STORAGE_RESOURCE) ?? DEFAULT_RESOURCE
  );

  const [showFunctions, setShowFunctions] = useState(false);
  const editorRef = useRef<CqlExpressionEditorHandle>(null);

  useEffect(() => {
    localStorage.setItem(STORAGE_EXPR, expression);
  }, [expression]);
  useEffect(() => {
    localStorage.setItem(STORAGE_RESOURCE, contextResource);
  }, [contextResource]);

  // Derive the context resource type so we can pass it to the server as the CQL
  // evaluation context (e.g. `context Patient`).
  const contextType = useMemo(() => {
    if (!contextResource.trim()) return undefined;
    try {
      const parsed = JSON.parse(contextResource);
      return typeof parsed?.resourceType === "string" ? parsed.resourceType : undefined;
    } catch {
      return undefined;
    }
  }, [contextResource]);

  const evaluateMutation = useMutation<CqlEvaluationResult, Error>({
    mutationFn: async () => {
      const body: {
        resourceType: string;
        parameter: Array<{
          name: string;
          valueString?: string;
          valueCode?: string;
          resource?: unknown;
        }>;
      } = {
        resourceType: "Parameters",
        parameter: [{ name: "expression", valueString: expression }],
      };

      if (contextResource.trim()) {
        let resource: unknown;
        try {
          resource = assertFhirResource(JSON.parse(contextResource), "CQL context resource");
        } catch {
          throw new Error("Invalid JSON in context resource");
        }
        if (contextType) {
          body.parameter.push({ name: "context", valueCode: contextType });
        }
        body.parameter.push({ name: "contextValue", resource });
      }

      const response = await fetch("/fhir/$cql", {
        method: "POST",
        headers: { "Content-Type": "application/fhir+json" },
        body: JSON.stringify(body),
      });

      if (!response.ok) {
        const error = await response.text();
        throw new Error(`HTTP ${response.status}: ${error}`);
      }

      const params = await response.json();
      return parseCqlResponse(params);
    },
  });

  const handleExecute = () => evaluateMutation.mutate();

  const handleClear = () => {
    setExpression("");
    evaluateMutation.reset();
  };

  const loadSample = (key: SampleKey) => setContextResource(sampleJsonString(key));

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
      setExpression((prev) => prev + snippet.replace(/\$\{\d+:?([^}]*)\}/g, "$1"));
    }
  };

  const data = evaluateMutation.data;

  return (
    <ToolWorkspaceLayout
      title="CQL Console"
      description="Evaluate Clinical Quality Language (CQL) expressions against an optional resource context"
      className="page-enter"
      actions={
        <div className={classes.actions}>
          <button
            type="button"
            className={classes.runBtn}
            onClick={handleExecute}
            disabled={evaluateMutation.isPending || !expression.trim()}
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
          {/* ── Expression ── */}
          <Resizable.Pane defaultSize={30} minSize={16}>
            <div className={classes.editorPanel}>
              <div className={classes.panelHead}>
                <span className={classes.panelTitle}>
                  <span className={classes.panelTitleIcon}>
                    <SquareFunction size={14} />
                  </span>
                  Expression
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
                  {EXPRESSION_EXAMPLES.map((ex) => (
                    <button
                      key={ex.label}
                      type="button"
                      className={classes.chip}
                      title={ex.expression}
                      onClick={() => {
                        setExpression(ex.expression);
                        if (ex.sample) loadSample(ex.sample);
                      }}
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
                <CqlExpressionEditor
                  ref={editorRef}
                  value={expression}
                  onChange={setExpression}
                  onSubmit={handleExecute}
                  height="100%"
                />
              </div>
            </div>
          </Resizable.Pane>

          <Resizable.Handle />

          {/* ── Context resource (optional) ── */}
          <Resizable.Pane defaultSize={33} minSize={14}>
            <div className={classes.editorPanel}>
              <div className={classes.panelHead}>
                <span className={classes.panelTitle}>
                  <span className={classes.panelTitleIcon}>
                    <Braces size={14} />
                  </span>
                  Context Resource
                  <span className={classes.chipRowLabel} style={{ textTransform: "none" }}>
                    optional
                  </span>
                </span>
                <span className={classes.panelHeadSpacer} />
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
              </div>
              <div className={classes.editorHost}>
                <JsonEditor
                  value={contextResource}
                  onChange={setContextResource}
                  onExecute={handleExecute}
                  height="100%"
                />
              </div>
            </div>
          </Resizable.Pane>

          <Resizable.Handle />

          {/* ── Result ── */}
          <Resizable.Pane defaultSize={37} minSize={16}>
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
                    <span className={classes.sectionTitle}>Result</span>
                    <span className={classes.sectionCount}>{data.datatype}</span>
                  </div>
                  <ResultItem result={data} />
                </div>
              ) : (
                !evaluateMutation.error && (
                  <div className={classes.emptyState}>
                    <span className={classes.emptyIcon}>
                      <SquareFunction size={24} />
                    </span>
                    <span className={classes.emptyTitle}>Ready to evaluate</span>
                    <span className={classes.emptyHint}>
                      Pick an example or type a CQL expression, then press <code>{RUN_KEY}</code> to
                      see the result.
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
