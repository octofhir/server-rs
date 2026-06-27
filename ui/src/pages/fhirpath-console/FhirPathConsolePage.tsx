import { Resizable } from "@octofhir/ui-kit";
import { useMutation } from "@tanstack/react-query";
import {
  Braces,
  CircleAlert,
  FunctionSquare,
  Play,
  Sparkles,
  Wand2,
  X as Xmark,
} from "lucide-react";
import { useEffect, useState } from "react";
import { assertFhirResource } from "@/shared/api/guards";
import { FhirPathEditor } from "@/shared/monaco/FhirPathEditor";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import { ResultItem } from "./components/ResultItem";
import classes from "./FhirPathConsolePage.module.css";
import { EXPRESSION_EXAMPLES, SAMPLE_RESOURCES, type SampleKey, sampleJsonString } from "./presets";
import { type FhirPathEvaluationResponse, parseParametersResponse } from "./types";

const STORAGE_EXPR = "octofhir.fhirpath.expression";
const STORAGE_RESOURCE = "octofhir.fhirpath.resource";

const DEFAULT_EXPRESSION = "Patient.name.given";
const DEFAULT_RESOURCE = sampleJsonString("patient");

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
const RUN_KEY = isMac ? "⌘↵" : "Ctrl+↵";

export function FhirPathConsolePage() {
  const [expression, setExpression] = useState(
    () => localStorage.getItem(STORAGE_EXPR) ?? DEFAULT_EXPRESSION
  );
  const [inputResource, setInputResource] = useState(
    () => localStorage.getItem(STORAGE_RESOURCE) ?? DEFAULT_RESOURCE
  );

  useEffect(() => {
    localStorage.setItem(STORAGE_EXPR, expression);
  }, [expression]);
  useEffect(() => {
    localStorage.setItem(STORAGE_RESOURCE, inputResource);
  }, [inputResource]);

  const evaluateMutation = useMutation<FhirPathEvaluationResponse, Error>({
    mutationFn: async () => {
      const body: {
        resourceType: string;
        parameter: Array<{
          name: string;
          valueString?: string;
          resource?: unknown;
        }>;
      } = {
        resourceType: "Parameters",
        parameter: [{ name: "expression", valueString: expression }],
      };

      if (inputResource.trim()) {
        try {
          const resource = assertFhirResource(JSON.parse(inputResource), "FHIRPath input resource");
          body.parameter.push({ name: "resource", resource });
        } catch {
          throw new Error("Invalid JSON in input resource");
        }
      }

      const response = await fetch("/fhir/$fhirpath", {
        method: "POST",
        headers: { "Content-Type": "application/fhir+json" },
        body: JSON.stringify(body),
      });

      if (!response.ok) {
        const error = await response.text();
        throw new Error(`HTTP ${response.status}: ${error}`);
      }

      const params = await response.json();
      return parseParametersResponse(params);
    },
  });

  const handleExecute = () => evaluateMutation.mutate();

  const handleClear = () => {
    setExpression("");
    evaluateMutation.reset();
  };

  const loadSample = (key: SampleKey) => setInputResource(sampleJsonString(key));

  const formatResource = () => {
    try {
      setInputResource(JSON.stringify(JSON.parse(inputResource), null, 2));
    } catch {
      /* leave invalid JSON untouched */
    }
  };

  const activeSampleKey = SAMPLE_RESOURCES.find(
    (s) => sampleJsonString(s.key) === inputResource
  )?.key;

  const data = evaluateMutation.data;

  return (
    <ToolWorkspaceLayout
      title="FHIRPath Console"
      description="Evaluate FHIRPath expressions against a sample or pasted resource"
      className="page-enter"
      actions={
        <div className={classes.actions}>
          <button
            type="button"
            className={classes.runBtn}
            onClick={handleExecute}
            disabled={evaluateMutation.isPending}
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
          <Resizable.Pane defaultSize={26} minSize={14}>
            <div className={classes.editorPanel}>
              <div className={classes.panelHead}>
                <span className={classes.panelTitle}>
                  <span className={classes.panelTitleIcon}>
                    <FunctionSquare size={14} />
                  </span>
                  Expression
                </span>
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
                </div>
              </div>
              <div className={classes.editorHost}>
                <FhirPathEditor
                  value={expression}
                  onChange={setExpression}
                  onSubmit={handleExecute}
                  height="100%"
                  placeholder="Enter FHIRPath expression (e.g., Patient.name.given)"
                />
              </div>
            </div>
          </Resizable.Pane>

          <Resizable.Handle />

          {/* ── Input resource ── */}
          <Resizable.Pane defaultSize={37} minSize={18}>
            <div className={classes.editorPanel}>
              <div className={classes.panelHead}>
                <span className={classes.panelTitle}>
                  <span className={classes.panelTitleIcon}>
                    <Braces size={14} />
                  </span>
                  Input Resource
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
                  value={inputResource}
                  onChange={setInputResource}
                  onExecute={handleExecute}
                  height="100%"
                />
              </div>
            </div>
          </Resizable.Pane>

          <Resizable.Handle />

          {/* ── Results ── */}
          <Resizable.Pane defaultSize={37} minSize={18}>
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
                <>
                  <div className={classes.statStrip}>
                    <div className={`${classes.statCard} ${classes.statCardAccent}`}>
                      <span className={classes.statLabel}>Results</span>
                      <span className={`${classes.statValue} ${classes.statValueAccent}`}>
                        {data.metadata.resultCount}
                      </span>
                    </div>
                    <div className={classes.statCard}>
                      <span className={classes.statLabel}>Total</span>
                      <span className={classes.statValue}>
                        {data.metadata.timing.totalTime.toFixed(2)}
                        <span style={{ fontSize: 11, fontWeight: 500 }}>ms</span>
                      </span>
                    </div>
                    <div className={classes.statMeta}>
                      <span>Evaluator</span>
                      <span className={classes.statMetaMono}>{data.metadata.evaluator}</span>
                    </div>
                  </div>

                  <div className={classes.metaTiming}>
                    <span className={classes.timingTag}>
                      Parse
                      <span className={classes.timingTagVal}>
                        {data.metadata.timing.parseTime.toFixed(2)}ms
                      </span>
                    </span>
                    <span className={classes.timingTag}>
                      Eval
                      <span className={classes.timingTagVal}>
                        {data.metadata.timing.evaluationTime.toFixed(2)}ms
                      </span>
                    </span>
                    <span className={classes.timingTag}>
                      Total
                      <span className={classes.timingTagVal}>
                        {data.metadata.timing.totalTime.toFixed(2)}ms
                      </span>
                    </span>
                  </div>

                  <div className={classes.resultSection}>
                    <div className={classes.resultSectionHead}>
                      <span className={classes.sectionTitle}>Results</span>
                      <span className={classes.sectionCount}>{data.results.length}</span>
                    </div>
                    {data.results.length === 0 ? (
                      <div className={classes.emptyHint}>
                        No results — the expression returned an empty collection.
                      </div>
                    ) : (
                      <div className={classes.resultList}>
                        {data.results.map((result) => (
                          <ResultItem key={result.index} result={result} />
                        ))}
                      </div>
                    )}
                  </div>
                </>
              ) : (
                !evaluateMutation.error && (
                  <div className={classes.emptyState}>
                    <span className={classes.emptyIcon}>
                      <FunctionSquare size={24} />
                    </span>
                    <span className={classes.emptyTitle}>Ready to evaluate</span>
                    <span className={classes.emptyHint}>
                      Pick an example or type an expression, then press <code>{RUN_KEY}</code> to
                      see results, timing, and types.
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
