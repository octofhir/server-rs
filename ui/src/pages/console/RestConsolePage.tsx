import { useDisclosure, useHotkeys } from "@octofhir/ui-kit";
import { Eye, ClockArrowRotateLeft, Play } from "@gravity-ui/icons";
import { useUnit } from "effector-react";
import { useCallback, useMemo } from "react";
import { Helmet } from "react-helmet-async";
import type { QueryInputMetadata } from "@/shared/fhir-query-input";
import { computeDiagnostics, parseQueryAst } from "@/shared/fhir-query-input";
import { QueryInspector } from "@/shared/fhir-query-input/widgets/QueryInspector";
import { Box, Button, Collapse, Flex, Stack, Text } from "@/shared/ui";
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
        <Flex gap="2">
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
        </Flex>
      }
    >
      <Helmet>
        <title>REST Console</title>
      </Helmet>

          <Stack gap="6">
            {/* Request Editor Section */}
            <Box style={{ 
              backgroundColor: "var(--g-color-base-background)", 
              borderRadius: "8px",
              border: "1px solid var(--g-color-line-generic-subtle)",
              overflow: "hidden",
            }}>
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
                <Box style={{ padding: "16px" }}>
                   <BuilderModeEditor
                    allSuggestions={allSuggestions}
                    searchParamsByResource={searchParamsByResource}
                    capabilities={data}
                    isLoading={isPending}
                  />
                  <Flex justifyContent="flex-end" mt="4">
                    <Button
                      size="l"
                      view="action"
                      onClick={handleSend}
                      loading={sendMutation.isPending}
                    >
                      <Button.Icon><Play size={18} /></Button.Icon>
                      Execute Request
                    </Button>
                  </Flex>
                </Box>
              )}
              
              {/* Additional request options (Tabs, Headers, Body) */}
              <Box style={{ borderTop: "1px solid var(--g-color-line-generic-subtle)" }}>
                <RequestOptionTabs />
              </Box>
            </Box>

            {/* Inspector (collapsible) */}
            <Collapse in={inspectorOpened}>
              <Box style={{ 
                padding: "16px",
                border: "1px solid var(--g-color-line-info-subtle)", 
                borderRadius: "8px",
                backgroundColor: "var(--g-color-base-info-subtle)" 
              }}>
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
              </Box>
            </Collapse>

            {/* Response Section */}
            <Box>
              {sendMutation.data || sendMutation.isPending ? (
                <Stack gap="3">
                  <Text variant="caption-1" style={{ textTransform: "uppercase", fontWeight: 700, color: "var(--g-color-text-secondary)" }}>
                    Response
                  </Text>
                  <Box style={{ 
                    borderRadius: "8px",
                    border: "1px solid var(--g-color-line-generic-subtle)",
                    backgroundColor: "var(--g-color-base-background)",
                    overflow: "hidden",
                  }}>
                    <ResponseViewer response={sendMutation.data} isLoading={sendMutation.isPending} />
                  </Box>
                </Stack>
              ) : (
                <Flex direction="column" alignItems="center" style={{ padding: "60px 0", opacity: 0.5 }}>
                  <Text variant="body-2">Execute a request to see the response payload here.</Text>
                </Flex>
              )}
            </Box>
          </Stack>

      <CommandPalette />
      <HistoryPanel opened={historyOpened} onClose={historyHandlers.close} />
    </ToolWorkspaceLayout>
  );
}
