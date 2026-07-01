import { ActionIcon, Collapse, Text, Tooltip, useDisclosure, useHotkeys } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { Bookmark, History as ClockArrowRotateLeft, Eye, Gauge, Inbox, Layers } from "lucide-react";
import { useCallback, useMemo } from "react";
import { Helmet } from "react-helmet-async";
import type { QueryInputMetadata } from "@/shared/fhir-query-input";
import { computeDiagnostics, parseQueryAst } from "@/shared/fhir-query-input";
import { QueryInspector } from "@/shared/fhir-query-input/widgets/QueryInspector";
import { CollectionsPanel } from "./components/CollectionsPanel";
import { CommandPalette } from "./components/CommandPalette";
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
  setCommandPaletteOpen,
} from "./state/consoleStore";

export function RestConsolePage() {
  const { isPending, data, resourceTypes, allSuggestions, searchParamsByResource } =
    useRestConsoleMeta();

  const {
    method,
    rawPath,
    body,
    customHeaders,
    resourceType,
    setCommandPaletteOpen: openPalette,
  } = useUnit({
    method: $method,
    rawPath: $rawPath,
    body: $body,
    customHeaders: $customHeaders,
    resourceType: $resourceType,
    setCommandPaletteOpen,
  });

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

  const handleOpenPalette = useCallback(
    (e: KeyboardEvent) => {
      e.preventDefault();
      openPalette(true);
    },
    [openPalette]
  );

  const hotkeys = useMemo(
    () =>
      [
        ["mod+K", handleOpenPalette],
        ["mod+Enter", handleSend],
      ] as const,
    [handleOpenPalette, handleSend]
  );

  useHotkeys(hotkeys);

  const hasResponse = Boolean(sendMutation.data) || sendMutation.isPending;

  return (
    <div className={`${styles.container} page-enter`}>
      <Helmet>
        <title>REST Console</title>
      </Helmet>

      <header className={styles.toolbar}>
        <div className={styles.toolbarLeft}>
          <Text variant="header-2" className={styles.title}>
            REST Console
          </Text>
        </div>
        <div className={styles.toolbarActions}>
          <Tooltip label={activeEnv ? `Environment: ${activeEnv.name}` : "Environment"}>
            <span className={styles.envWrap}>
              <ActionIcon variant="subtle" size="md" onClick={envHandlers.open}>
                <Layers size={16} />
              </ActionIcon>
              {activeEnv && <span className={styles.envDot} />}
            </span>
          </Tooltip>
          <Tooltip label="Saved requests">
            <ActionIcon variant="subtle" size="md" onClick={savedHandlers.open}>
              <Bookmark size={16} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label="Explain query">
            <ActionIcon variant="subtle" size="md" onClick={explainHandlers.open}>
              <Gauge size={16} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label="Inspector">
            <ActionIcon
              variant={inspectorOpened ? "light" : "subtle"}
              size="md"
              onClick={toggleInspector}
            >
              <Eye size={16} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label="History">
            <ActionIcon variant="subtle" size="md" onClick={historyHandlers.open}>
              <ClockArrowRotateLeft size={16} />
            </ActionIcon>
          </Tooltip>
        </div>
      </header>

      <div className={styles.workspace}>
        {/* Request bar — always the smart query editor, always visible */}
        <section className={styles.requestRegion}>
          <RequestBar
            allSuggestions={allSuggestions}
            searchParamsByResource={searchParamsByResource}
            capabilities={data}
            isLoading={isPending}
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
              <Inbox size={30} className={styles.emptyIcon} />
              <Text variant="body-1">No response yet</Text>
              <Text variant="body-2" color="secondary">
                Build a request and press <span style={{ fontWeight: 600 }}>⌘ Enter</span> to run
                it.
              </Text>
            </div>
          )}
        </div>
      </div>

      <CommandPalette />
      <HistoryPanel opened={historyOpened} onClose={historyHandlers.close} />
      <CollectionsPanel opened={savedOpened} onClose={savedHandlers.close} />
      <EnvironmentPanel opened={envOpened} onClose={envHandlers.close} />
      <ExplainPanel opened={explainOpened} onClose={explainHandlers.close} path={rawPath} />
    </div>
  );
}
