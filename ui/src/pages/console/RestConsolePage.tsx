import { Button, Collapse, Resizable, Text, useDisclosure, useHotkeys } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import {
  Bookmark,
  History as ClockArrowRotateLeft,
  Eye,
  Gauge,
  Inbox,
  Layers,
  Play,
} from "lucide-react";
import { useCallback, useMemo } from "react";
import { Helmet } from "react-helmet-async";
import type { QueryInputMetadata } from "@/shared/fhir-query-input";
import { computeDiagnostics, parseQueryAst } from "@/shared/fhir-query-input";
import { QueryInspector } from "@/shared/fhir-query-input/widgets/QueryInspector";
import { BuilderModeEditor } from "./components/BuilderModeEditor";
import { CollectionsPanel } from "./components/CollectionsPanel";
import { CommandPalette } from "./components/CommandPalette";
import { EnvironmentPanel } from "./components/EnvironmentPanel";
import { ExplainPanel } from "./components/ExplainPanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { ModeControl } from "./components/ModeControl";
import { RequestBar } from "./components/RequestBar";
import { RequestOptionTabs } from "./components/RequestBuilderAccordion";
import { ResponseViewer } from "./components/ResponseViewer";
import { useEnvironments } from "./hooks/useEnvironments";
import { useRestConsoleMeta } from "./hooks/useRestConsoleMeta";
import { useSendConsoleRequest } from "./hooks/useSendConsoleRequest";
import styles from "./RestConsolePage.module.css";
import {
  $body,
  $customHeaders,
  $method,
  $mode,
  $rawPath,
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
    mode,
    setCommandPaletteOpen: openPalette,
  } = useUnit({
    method: $method,
    rawPath: $rawPath,
    body: $body,
    customHeaders: $customHeaders,
    mode: $mode,
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
          <ModeControl />
        </div>
        <div className={styles.toolbarActions}>
          <Button variant="subtle" size="md" onClick={envHandlers.open}>
            <Button.Icon>
              <Layers size={16} />
            </Button.Icon>
            {activeEnv ? activeEnv.name : "Env"}
          </Button>
          <Button variant="subtle" size="md" onClick={savedHandlers.open}>
            <Button.Icon>
              <Bookmark size={16} />
            </Button.Icon>
            Saved
          </Button>
          <Button variant="subtle" size="md" onClick={explainHandlers.open}>
            <Button.Icon>
              <Gauge size={16} />
            </Button.Icon>
            Explain
          </Button>
          <Button variant="subtle" size="md" onClick={toggleInspector}>
            <Button.Icon>
              <Eye size={16} />
            </Button.Icon>
            Inspector
          </Button>
          <Button variant="subtle" size="md" onClick={historyHandlers.open}>
            <Button.Icon>
              <ClockArrowRotateLeft size={16} />
            </Button.Icon>
            History
          </Button>
        </div>
      </header>

      <div className={styles.workspace}>
        {/* Request bar / builder — always visible at the top */}
        <section className={styles.requestRegion}>
          {mode === "pro" ? (
            <RequestBar
              allSuggestions={allSuggestions}
              searchParamsByResource={searchParamsByResource}
              capabilities={data}
              isLoading={isPending}
              isSending={sendMutation.isPending}
              onSend={handleSend}
            />
          ) : (
            <div className={styles.builderContent}>
              <BuilderModeEditor
                allSuggestions={allSuggestions}
                searchParamsByResource={searchParamsByResource}
                capabilities={data}
                isLoading={isPending}
              />
              <div className={styles.builderActions}>
                <Button
                  size="lg"
                  variant="filled"
                  onClick={handleSend}
                  loading={sendMutation.isPending}
                >
                  <Button.Icon>
                    <Play size={18} />
                  </Button.Icon>
                  Execute Request
                </Button>
              </div>
            </div>
          )}
        </section>

        {/* Split: request options (top) ⇄ response (bottom) */}
        <div className={styles.split}>
          <Resizable.Group orientation="vertical">
            <Resizable.Pane defaultSize={42} minSize={20}>
              <div className={styles.panel}>
                <div className={styles.panelBody}>
                  <RequestOptionTabs />
                </div>
              </div>
            </Resizable.Pane>

            <Resizable.Handle />

            <Resizable.Pane defaultSize={58} minSize={20}>
              <div className={styles.panel}>
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
                    <ResponseViewer
                      response={sendMutation.data}
                      isLoading={sendMutation.isPending}
                    />
                  </div>
                ) : (
                  <div className={styles.emptyResponse}>
                    <Inbox size={32} className={styles.emptyIcon} />
                    <Text variant="body-1">No response yet</Text>
                    <Text variant="body-2" color="secondary">
                      Build a request and press <span style={{ fontWeight: 600 }}>⌘ Enter</span> to
                      run it.
                    </Text>
                  </div>
                )}
              </div>
            </Resizable.Pane>
          </Resizable.Group>
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
