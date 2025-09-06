import { Badge, Card, Group, ScrollArea, Stack, Text, Tooltip } from "@mantine/core";
import { useUnit } from "effector-react";
import { useMemo, useState } from "react";
import { JsonViewer } from "@/shared/ui";
import { $responseState } from "../model/store";

function statusColor(status?: number): string {
  if (!status) return "gray";
  if (status >= 200 && status < 300) return "green";
  if (status >= 300 && status < 400) return "yellow";
  if (status >= 400 && status < 500) return "orange";
  return "red";
}

export function ResponseViewer() {
  const state = useUnit($responseState);
  const [showRaw, setShowRaw] = useState(false);

  const headersList = useMemo(
    () => Object.entries(state.response?.headers ?? {}),
    [state.response]
  );

  const dataForViewer = useMemo(() => {
    if (!state.response) return null;
    const data = state.response.data;
    if (showRaw) return typeof data === "string" ? data : JSON.stringify(data);
    return data;
  }, [state.response, showRaw]);

  return (
    <Card withBorder radius="md" p="md">
      <Stack gap="sm">
        <Group justify="space-between" wrap="wrap">
          <Group gap="sm">
            <Text fw={600}>Response</Text>
            {state.response && (
              <Badge color={statusColor(state.response.status)}>
                {state.response.status} {state.response.statusText}
              </Badge>
            )}
          </Group>
          <Group gap="md">
            <Text size="sm" c="dimmed">
              {state.durationMs != null ? `${state.durationMs} ms` : "-"}
            </Text>
            <Text size="sm" c="dimmed">
              {state.sizeBytes != null ? `${state.sizeBytes} B` : "-"}
            </Text>
            <Badge variant="light" onClick={() => setShowRaw((v) => !v)} style={{ cursor: "pointer" }}>
              {showRaw ? "Raw" : "JSON"}
            </Badge>
          </Group>
        </Group>

        {state.loading && <Text size="sm">Loading...</Text>}
        {state.error && (
          <Text c="red" size="sm">
            {state.error}
          </Text>
        )}

        {/* Headers */}
        {headersList.length > 0 && (
          <Stack gap={4}>
            <Text fw={600} size="sm">
              Headers
            </Text>
            <ScrollArea h={100} offsetScrollbars>
              <Stack gap={2}
                styles={{
                  root: { border: "1px solid var(--mantine-color-gray-3)", borderRadius: 8, padding: 8 },
                }}
              >
                {headersList.map(([k, v]) => (
                  <Group key={k} gap={8} wrap="nowrap">
                    <Tooltip label={k}><Text fw={600} size="xs" style={{ width: 200, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{k}</Text></Tooltip>
                    <Text size="xs" c="dimmed" style={{ whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{v}</Text>
                  </Group>
                ))}
              </Stack>
            </ScrollArea>
          </Stack>
        )}

        {/* Body */}
        {state.response && (
          <Stack gap={4}>
            <Text fw={600} size="sm">
              Body
            </Text>
            <ScrollArea h={240} offsetScrollbars>
              {typeof dataForViewer === "string" ? (
                <pre style={{ margin: 0 }}>{dataForViewer}</pre>
              ) : (
                <JsonViewer data={dataForViewer} />
              )}
            </ScrollArea>
          </Stack>
        )}
      </Stack>
    </Card>
  );
}
