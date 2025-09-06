import {
  ActionIcon,
  Button,
  Card,
  Group,
  Select,
  Stack,
  Text,
  TextInput,
  Textarea,
  Tooltip,
} from "@mantine/core";
import { useHotkeys } from "@mantine/hooks";
import { useUnit } from "effector-react";
import { IconPlus, IconTrash } from "@tabler/icons-react";
import { useCallback, useMemo, useState } from "react";
import type { HttpMethod } from "@/shared/api/types";
import { $apiBaseUrl, $apiTimeout } from "@/entities/settings/model";
import {
  $restRequest,
  removeHeader,
  sendRequestFx,
  setBody,
  setCommonHeader,
  setHeader,
  setMethod,
  setPath,
} from "../model/store";
import { addHistoryItem } from "@/features/rest-console/model/history";
import { setResponseState, setResponseError } from "@/features/rest-response-viewer/model/store";

const METHOD_OPTIONS: { value: HttpMethod; label: string }[] = [
  { value: "GET", label: "GET" },
  { value: "POST", label: "POST" },
  { value: "PUT", label: "PUT" },
  { value: "DELETE", label: "DELETE" },
  { value: "PATCH", label: "PATCH" },
];

export function RequestBuilder() {
  const request = useUnit($restRequest);
  const [apiBaseUrl, apiTimeout] = useUnit([$apiBaseUrl, $apiTimeout]);
  const isSending = useUnit(sendRequestFx.pending);
  const [newHeaderKey, setNewHeaderKey] = useState("");
  const [newHeaderValue, setNewHeaderValue] = useState("");

  const onSend = useCallback(async () => {
    try {
      setResponseState({ loading: true });
      const result = await sendRequestFx({
        request,
        baseUrl: apiBaseUrl,
        timeout: apiTimeout,
      });

      setResponseState({
        loading: false,
        response: result.response,
        durationMs: result.durationMs,
        sizeBytes: result.sizeBytes,
      });

      addHistoryItem({
        id: crypto.randomUUID(),
        timestamp: new Date().toISOString(),
        method: request.method,
        path: request.path,
        status: result.response.status,
        duration: result.durationMs,
        success: result.response.status >= 200 && result.response.status < 300,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : "Unknown error";
      setResponseError(message);
      setResponseState({ loading: false });
    }
  }, [apiBaseUrl, apiTimeout, request]);

  useHotkeys([
    ["mod+Enter", (e) => {
      e.preventDefault();
      onSend();
    }],
  ]);

  const headersList = useMemo(() => Object.entries(request.headers), [request.headers]);

  return (
    <Card withBorder radius="md" p="md">
      <Stack gap="md">
        <Group align="flex-end" gap="sm" wrap="nowrap">
          <Select
            data={METHOD_OPTIONS}
            value={request.method}
            onChange={(v) => v && setMethod(v as HttpMethod)}
            w={120}
            allowDeselect={false}
          />
          <TextInput
            label="Path"
            placeholder="/Patient/123 or /metadata"
            value={request.path}
            onChange={(e) => setPath(e.currentTarget.value)}
            style={{ flex: 1 }}
          />
          <Button onClick={onSend} loading={isSending}>
            Send
          </Button>
        </Group>

        <Group gap="xs">
          <Text fw={600}>Headers</Text>
          <Tooltip label="Add Accept: application/fhir+json">
            <Button size="xs" variant="light" onClick={() => setCommonHeader("Accept")}>Add Accept</Button>
          </Tooltip>
          <Tooltip label="Add Content-Type: application/fhir+json">
            <Button size="xs" variant="light" onClick={() => setCommonHeader("Content-Type")}>
              Add Content-Type
            </Button>
          </Tooltip>
        </Group>

        {/* Headers editor */}
        <Stack gap="xs">
          {headersList.length === 0 && (
            <Text c="dimmed" size="sm">No headers</Text>
          )}
          {headersList.map(([key, value]) => (
            <Group key={key} gap="xs">
              <TextInput value={key} readOnly w={240} />
              <TextInput value={value} onChange={(e) => setHeader({ key, value: e.currentTarget.value })} style={{ flex: 1 }} />
              <ActionIcon variant="subtle" color="red" aria-label="Remove header" onClick={() => removeHeader(key)}>
                <IconTrash size={18} />
              </ActionIcon>
            </Group>
          ))}
          <Group gap="xs">
            <TextInput placeholder="Header name" value={newHeaderKey} onChange={(e) => setNewHeaderKey(e.currentTarget.value)} w={240} />
            <TextInput placeholder="Header value" value={newHeaderValue} onChange={(e) => setNewHeaderValue(e.currentTarget.value)} style={{ flex: 1 }} />
            <ActionIcon
              variant="subtle"
              aria-label="Add header"
              onClick={() => {
                if (!newHeaderKey) return;
                setHeader({ key: newHeaderKey, value: newHeaderValue });
                setNewHeaderKey("");
                setNewHeaderValue("");
              }}
            >
              <IconPlus size={18} />
            </ActionIcon>
          </Group>
        </Stack>

        <Textarea
          label="Body (JSON)"
          placeholder="{}"
          minRows={6}
          autosize
          value={request.body}
          onChange={(e) => setBody(e.currentTarget.value)}
        />
      </Stack>
    </Card>
  );
}
