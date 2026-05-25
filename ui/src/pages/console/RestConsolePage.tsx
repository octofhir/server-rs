import { Button, Collapse, Text, useDisclosure, useHotkeys, Resizable } from "@octofhir/ui-kit";
import { Eye, ClockArrowRotateLeft, Play } from "@gravity-ui/icons";
import { useUnit } from "effector-react";
import { useCallback, useMemo } from "react";
import { Helmet } from "react-helmet-async";
import type { QueryInputMetadata } from "@/shared/fhir-query-input";
import { computeDiagnostics, parseQueryAst } from "@/shared/fhir-query-input";
import { QueryInspector } from "@/shared/fhir-query-input/widgets/QueryInspector";
import { BuilderModeEditor } from "./components/BuilderModeEditor";
import { CommandPalette } from "./components/CommandPalette";
import { HistoryPanel } from "./components/HistoryPanel";
import { ModeControl } from "./components/ModeControl";
import { RequestBar } from "./components/RequestBar";
import { RequestOptionTabs } from "./components/RequestBuilderAccordion";
import { ResponseViewer } from "./components/ResponseViewer";
import { useRestConsoleMeta } from "./hooks/useRestConsoleMeta";
import { useSendConsoleRequest } from "./hooks/useSendConsoleRequest";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import {
  $body,
  $customHeaders,
  $method,
  $mode,
  $rawPath,
  setCommandPaletteOpen,
} from "./state/consoleStore";
import styles from "./RestConsolePage.module.css";

export function RestConsolePage() {
  const { isPending, data, resourceTypes, allSuggestions, searchParamsByResource } =
    useRestConsoleMeta();

  // Console state
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

  // Send request mutation
  const sendMutation = useSendConsoleRequest();

  // History drawer
  const [historyOpened, historyHandlers] = useDisclosure(false);

  // Inspector
  const [inspectorOpened, { toggle: toggleInspector }] = useDisclosure(false);

  // Metadata for inspector
  const inspectorMetadata: QueryInputMetadata = useMemo(
    () => ({
      resourceTypes,
      searchParamsByResource,
      allSuggestions,
      capabilities: data,
    }),
    [resourceTypes, searchParamsByResource, allSuggestions, data]
  );

  // Parse AST and diagnostics for inspector
  const inspectorAst = useMemo(() => parseQueryAst(rawPath || "/fhir/"), [rawPath]);
  const inspectorDiagnostics = useMemo(
    () => computeDiagnostics(inspectorAst, inspectorMetadata),
    [inspectorAst, inspectorMetadata]
  );

  const handleSend = useCallback(() => {
    sendMutation.mutate({
      method,
      path: rawPath,
      body,
      headers: customHeaders,
    });
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

  return (
    <ToolWorkspaceLayout
      className="page-enter"
      title="REST Console"
      description="Build, send, and inspect FHIR REST requests"
      maxWidth={1280}
      toolbar={<ModeControl />}
      actions={
        <div className={styles.actions}>
          <Button
            view="flat"
            size="m"
            onClick={toggleInspector}
          >
            <Button.Icon><Eye size={16} /></Button.Icon>
            Inspector
          </Button>
          <Button
            view="flat"
            size="m"
            onClick={historyHandlers.open}
          >
            <Button.Icon><ClockArrowRotateLeft size={16} /></Button.Icon>
            History
          </Button>
        </div>
      }
    >
      <Helmet>
        <title>REST Console</title>
      </Helmet>

          <div className={styles.rootResizable}>
            <Resizable.Group orientation="vertical">
              <Resizable.Pane defaultSize={50} minSize={25}>
                <section className={styles.requestPanel} style={{ height: "100%", overflow: "auto" }}>
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
                          size="l"
                          view="action"
                          onClick={handleSend}
                          loading={sendMutation.isPending}
                        >
                          <Button.Icon><Play size={18} /></Button.Icon>
                          Execute Request
                        </Button>
                      </div>
                    </div>
                  )}
                  
                  {/* Additional request options (Tabs, Headers, Body) */}
                  <div className={styles.requestOptions}>
                    <RequestOptionTabs />
                  </div>
                </section>
              </Resizable.Pane>

              <Resizable.Handle />

              <Resizable.Pane defaultSize={50} minSize={25}>
                <div style={{ height: "100%", display: "flex", flexDirection: "column", minHeight: 0 }}>
                  {/* Inspector (collapsible) */}
                  <Collapse in={inspectorOpened}>
                    <section className={styles.inspectorPanel} style={{ marginBottom: 12 }}>
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

                  {/* Response Section */}
                  <section style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
                    {sendMutation.data || sendMutation.isPending ? (
                      <div className={styles.responseSection} style={{ height: "100%", display: "flex", flexDirection: "column", minHeight: 0 }}>
                        <Text variant="caption-1" className={styles.sectionLabel}>
                          Response
                        </Text>
                        <div className={styles.responsePanel} style={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
                          <ResponseViewer response={sendMutation.data} isLoading={sendMutation.isPending} />
                        </div>
                      </div>
                    ) : (
                      <div className={styles.emptyResponse}>
                        <Text variant="body-2">Execute a request to see the response payload here.</Text>
                      </div>
                    )}
                  </section>
                </div>
              </Resizable.Pane>
            </Resizable.Group>
          </div>

      <CommandPalette />
      <HistoryPanel opened={historyOpened} onClose={historyHandlers.close} />
    </ToolWorkspaceLayout>
  );
}
