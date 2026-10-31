import { Button, Collapse, Text, useDisclosure, useHotkeys } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { Bookmark, History as ClockArrowRotateLeft, Eye, Gauge, Layers } from "lucide-react";
import { useCallback, useEffect, useMemo } from "react";
import { useSearchParams } from "react-router-dom";
import type { HttpMethod } from "@/shared/api";
import type { QueryInputMetadata } from "@/shared/fhir-query-input";
import { computeDiagnostics, parseQueryAst } from "@/shared/fhir-query-input";
import { QueryInspector } from "@/shared/fhir-query-input/widgets/QueryInspector";
import { usePaletteContribution } from "@/shared/command-palette";
import { CollectionsPanel } from "./components/CollectionsPanel";
import { useConsolePaletteItems } from "./commands/hooks/useConsolePaletteItems";
import { EnvironmentPanel } from "./components/EnvironmentPanel";
import { ExplainPanel } from "./components/ExplainPanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { RequestBar } from "./components/RequestBar";
import { RequestOptionsStrip } from "./components/RequestOptionsStrip";
import { ResponseViewer } from "./components/ResponseViewer";
import { useEnvironments } from "./hooks/useEnvironments";
import { useRestConsoleMeta } from "./hooks/useRestConsoleMeta";
import { useSendConsoleRequest } from "./hooks/useSendConsoleRequest";
import styles from "./RestConsolePage.module.css";
import {
  $body,
  $customHeaders,
  $method,
  $rawPath,
  $resourceType,
  setMethod,
  setRawPath,
} from "./state/consoleStore";

const EXAMPLES: Array<{ method: HttpMethod; path: string; note: string }> = [
  { method: "GET", path: "/fhir/Patient?_count=20", note: "list patients" },
  { method: "GET", path: "/fhir/Observation?_sort=-_lastUpdated&_count=10", note: "latest observations" },
  { method: "GET", path: "/fhir/Patient?_summary=count", note: "count only" },
  { method: "GET", path: "/fhir/metadata", note: "capability statement" },
];

export function RestConsolePage() {
  const { data, resourceTypes, allSuggestions, searchParamsByResource } =
    useRestConsoleMeta();

  const {
    method,
    rawPath,
    body,
    customHeaders,
    resourceType,
  } = useUnit({
    method: $method,
    rawPath: $rawPath,
    body: $body,
    customHeaders: $customHeaders,
    resourceType: $resourceType,
  });
  const [chooseMethod, choosePath] = useUnit([setMethod, setRawPath]);
  usePaletteContribution("rest-console", useConsolePaletteItems());

  // `/console?url=/fhir/Patient/123` (from "Open in Requests") loads that GET into the bar.
  const [urlParams, setUrlParams] = useSearchParams();
  useEffect(() => {
    const url = urlParams.get("url");
    if (!url) return;
    chooseMethod("GET");
    choosePath(url);
    setUrlParams({}, { replace: true });
  }, [urlParams, setUrlParams, chooseMethod, choosePath]);

  const sendMutation = useSendConsoleRequest();
  // Keep the active-environment variable resolver in sync (even with panels closed).
  const { active: activeEnv } = useEnvironments();
  const [historyOpened, historyHandlers] = useDisclosure(false);
  const [savedOpened, savedHandlers] = useDisclosure(false);
  const [envOpened, envHandlers] = useDisclosure(false);
  const [explainOpened, explainHandlers] = useDisclosure(false);
  const [inspectorOpened, { toggle: toggleInspector }] = useDisclosure(false);

  const inspectorMetadata: QueryInputMetadata = useMemo(
    () => ({
      resourceTypes,
      searchParamsByResource,
      allSuggestions,
      capabilities: data,
    }),
    [resourceTypes, searchParamsByResource, allSuggestions, data]
  );

  const inspectorAst = useMemo(() => parseQueryAst(rawPath || "/fhir/"), [rawPath]);
  const inspectorDiagnostics = useMemo(
    () => computeDiagnostics(inspectorAst, inspectorMetadata),
    [inspectorAst, inspectorMetadata]
  );

  const handleSend = useCallback(() => {
    sendMutation.mutate({ method, path: rawPath, body, headers: customHeaders });
  }, [sendMutation, method, rawPath, body, customHeaders]);

  const hotkeys = useMemo(
    () =>
      [
        ["mod+Enter", handleSend],
      ] as const,
    [handleSend]
  );

  useHotkeys(hotkeys);

  const hasResponse = Boolean(sendMutation.data) || sendMutation.isPending;

  return (
    <div className={`${styles.container} page-enter`}>

      <header className={styles.toolbar}>
        <div className={styles.toolbarLeft}>
          <Text variant="header-2" className={styles.title}>
            Requests
          </Text>
        </div>
        <div className={styles.toolbarActions}>
          <Button size="sm" variant="subtle" onClick={historyHandlers.open}>
            <Button.Icon side="left">
              <ClockArrowRotateLeft size={14} />
            </Button.Icon>
            History
          </Button>
          <Button size="sm" variant="subtle" onClick={savedHandlers.open}>
            <Button.Icon side="left">
              <Bookmark size={14} />
            </Button.Icon>
            Saved
          </Button>
          <Button size="sm" variant="subtle" onClick={envHandlers.open}>
            <Button.Icon side="left">
              <Layers size={14} />
            </Button.Icon>
            {activeEnv ? activeEnv.name : "Environment"}
            {activeEnv && <span className={styles.envDot} aria-label="active" />}
          </Button>
          <span className={styles.divider} aria-hidden="true" />
          <Button size="sm" variant="subtle" onClick={explainHandlers.open}>
            <Button.Icon side="left">
              <Gauge size={14} />
            </Button.Icon>
            Explain
          </Button>
          <Button size="sm" variant={inspectorOpened ? "light" : "subtle"} onClick={toggleInspector}>
            <Button.Icon side="left">
              <Eye size={14} />
            </Button.Icon>
            Inspector
          </Button>
        </div>
      </header>

      <div className={styles.workspace}>
        {/* Request bar — always the smart query editor, always visible */}
        <section className={styles.requestRegion}>
          <RequestBar
            allSuggestions={allSuggestions}
            searchParamsByResource={searchParamsByResource}
            capabilities={data}
            isSending={sendMutation.isPending}
            onSend={handleSend}
          />
        </section>

        {/* Collapsible headers/body — collapsed by default to keep it compact */}
        <RequestOptionsStrip resourceType={resourceType} />

        {/* Response — the primary surface, fills all remaining space */}
        <div className={styles.responsePanel}>
          <Collapse in={inspectorOpened}>
            <section className={styles.inspectorPanel}>
              <QueryInspector
                ast={inspectorAst}
                diagnostics={inspectorDiagnostics}
                metadata={inspectorMetadata}
                response={
                  sendMutation.data
                    ? {
                        status: sendMutation.data.status,
                        statusText: sendMutation.data.statusText,
                        durationMs: sendMutation.data.durationMs,
                        body: sendMutation.data.body,
                        requestPath: sendMutation.data.requestPath,
                      }
                    : undefined
                }
              />
            </section>
          </Collapse>

          {hasResponse ? (
            <div className={styles.responseBody}>
              <ResponseViewer response={sendMutation.data} isLoading={sendMutation.isPending} />
            </div>
          ) : (
            <div className={styles.emptyResponse}>
              <Text variant="subheader-2">Start with a request</Text>
              <div className={styles.examples}>
                {EXAMPLES.map((example) => (
                  <button
                    key={`${example.method} ${example.path}`}
                    type="button"
                    className={styles.example}
                    onClick={() => {
                      chooseMethod(example.method);
                      choosePath(example.path);
                    }}
                  >
                    <span className={styles.exampleMethod} data-method={example.method}>
                      {example.method}
                    </span>
                    <span className={styles.examplePath}>{example.path}</span>
                    <span className={styles.exampleNote}>{example.note}</span>
                  </button>
                ))}
              </div>
              <Text variant="caption-1" color="secondary">
                <kbd className={styles.kbd}>⌘↵</kbd> send · <kbd className={styles.kbd}>⌘K</kbd> commands
                and recent requests
              </Text>
            </div>
          )}
        </div>
      </div>

      <HistoryPanel opened={historyOpened} onClose={historyHandlers.close} />
      <CollectionsPanel opened={savedOpened} onClose={savedHandlers.close} />
      <EnvironmentPanel opened={envOpened} onClose={envHandlers.close} />
      <ExplainPanel opened={explainOpened} onClose={explainHandlers.close} path={rawPath} />
    </div>
  );
}
