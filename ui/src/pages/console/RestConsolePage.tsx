import { Collapse } from "@mantine/core";
import { useDisclosure, useHotkeys } from "@mantine/hooks";
import { IconEye, IconHistory } from "@tabler/icons-react";
import { useUnit } from "effector-react";
import { useCallback, useMemo } from "react";
import { Helmet } from "react-helmet-async";
import type { QueryInputMetadata } from "@/shared/fhir-query-input";
import { computeDiagnostics, parseQueryAst } from "@/shared/fhir-query-input";
import { QueryInspector } from "@/shared/fhir-query-input/widgets/QueryInspector";
import { Box, Button, Group, Stack, Text, Title } from "@/shared/ui";
import { BuilderModeEditor } from "./components/BuilderModeEditor";
import { CommandPalette } from "./components/CommandPalette";
import { HistoryPanel } from "./components/HistoryPanel";
import { ModeControl } from "./components/ModeControl";
import { RequestBar } from "./components/RequestBar";
import { RequestOptionTabs } from "./components/RequestBuilderAccordion";
import { ResponseViewer } from "./components/ResponseViewer";
import { useRestConsoleMeta } from "./hooks/useRestConsoleMeta";
import { useSendConsoleRequest } from "./hooks/useSendConsoleRequest";
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
    <Box className="page-enter" p="xl" style={{ height: "100%", overflow: "auto" }}>
      <Helmet>
        <title>REST Console</title>
      </Helmet>

      <Box maw={960} mx="auto">
        <Stack gap="lg">
          {/* Header */}
          <Group justify="space-between" align="center">
            <Group gap="md" align="center">
              <Title order={2} style={{ letterSpacing: "-0.02em", fontWeight: 700 }}>
                REST Console
              </Title>
              <ModeControl />
            </Group>
            <Group gap="xs">
              <Button
                variant="subtle"
                size="xs"
                leftSection={<IconEye size={14} />}
                onClick={toggleInspector}
              >
                Inspector
              </Button>
              <Button
                variant="subtle"
                size="xs"
                leftSection={<IconHistory size={14} />}
                onClick={historyHandlers.open}
              >
                History
              </Button>
            </Group>
          </Group>

          {/* Request Bar (Pro mode) */}
          {mode === "pro" && (
            <RequestBar
              allSuggestions={allSuggestions}
              searchParamsByResource={searchParamsByResource}
              capabilities={data}
              isLoading={isPending}
              isSending={sendMutation.isPending}
              onSend={handleSend}
            />
          )}

          {/* Builder mode */}
          {mode === "builder" && (
            <Stack gap="sm">
              <BuilderModeEditor
                allSuggestions={allSuggestions}
                searchParamsByResource={searchParamsByResource}
                capabilities={data}
                isLoading={isPending}
              />
              <Group justify="flex-end">
                <Button
                  onClick={handleSend}
                  loading={sendMutation.isPending}
                >
                  Send
                </Button>
              </Group>
            </Stack>
          )}

          {/* Headers / Body tabs — collapsed by default */}
          <RequestOptionTabs />

          {/* Inspector (collapsible) */}
          <Collapse in={inspectorOpened}>
            <Box
              p="md"
              style={{
                border: "1px solid var(--app-border-subtle)",
                borderRadius: "var(--mantine-radius-md)",
                backgroundColor: "var(--app-surface-1)",
              }}
            >
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

          {/* Response */}
          <Box>
            {sendMutation.data || sendMutation.isPending ? (
              <Stack gap="xs">
                <Text size="xs" fw={600} c="dimmed" tt="uppercase">
                  Response
                </Text>
                <ResponseViewer response={sendMutation.data} isLoading={sendMutation.isPending} />
              </Stack>
            ) : (
              <Text size="sm" c="dimmed" ta="center" py="xl">
                Send a request to see the response
              </Text>
            )}
          </Box>
        </Stack>
      </Box>

      <CommandPalette />
      <HistoryPanel opened={historyOpened} onClose={historyHandlers.close} />
    </Box>
  );
}
