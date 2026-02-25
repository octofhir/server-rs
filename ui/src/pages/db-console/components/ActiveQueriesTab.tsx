import { modals, notifications } from "@octofhir/ui-kit";
import { IconActivity, IconPlayerStop } from "@tabler/icons-react";
import { useCallback } from "react";
import { useActiveQueries, useTerminateQuery } from "@/shared/api/hooks";
import type { ActiveQuery } from "@/shared/api/types";
import {
  ActionIcon,
  Badge,
  Box,
  Group,
  Loader,
  ScrollArea,
  Stack,
  Text,
  Tooltip,
} from "@/shared/ui";

interface ActiveQueriesTabProps {
  isActive: boolean;
}

function formatDuration(ms?: number): string {
  if (ms == null) return "-";
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60000).toFixed(1)}m`;
}

function stateColor(state?: string): string {
  switch (state) {
    case "active":
      return "primary";
    case "idle":
      return "deep";
    case "idle in transaction":
      return "warm";
    case "idle in transaction (aborted)":
      return "fire";
    default:
      return "gray";
  }
}

function QueryItem({
  query,
  onTerminate,
  isTerminating,
}: {
  query: ActiveQuery;
  onTerminate: (pid: number) => void;
  isTerminating: boolean;
}) {
  return (
    <Box
      p="xs"
      style={{
        borderBottom: "1px solid var(--octo-border-subtle)",
      }}
    >
      <Group justify="space-between" wrap="nowrap" mb={4}>
        <Group gap={6} wrap="nowrap">
          <Badge size="xs" variant="light" color={stateColor(query.state)}>
            {query.state ?? "unknown"}
          </Badge>
          <Text size="xs" c="dimmed">
            PID {query.pid}
          </Text>
        </Group>
        <Group gap={4} wrap="nowrap">
          <Text size="xs" c="dimmed">
            {formatDuration(query.durationMs)}
          </Text>
          <Tooltip label="Terminate query">
            <ActionIcon
              variant="subtle"
              size="xs"
              color="fire"
              onClick={() => onTerminate(query.pid)}
              loading={isTerminating}
            >
              <IconPlayerStop size={12} />
            </ActionIcon>
          </Tooltip>
        </Group>
      </Group>
      {query.query && (
        <Text size="xs" ff="monospace" lineClamp={3} style={{ wordBreak: "break-all" }}>
          {query.query}
        </Text>
      )}
      {query.username && (
        <Text size="xs" c="dimmed" mt={2}>
          {query.username}
        </Text>
      )}
    </Box>
  );
}

export function ActiveQueriesTab({ isActive }: ActiveQueriesTabProps) {
  const { data, isLoading } = useActiveQueries(isActive);
  const terminateMutation = useTerminateQuery();

  const handleTerminate = useCallback(
    (pid: number) => {
      modals.openConfirmModal({
        title: "Terminate Query",
        children: (
          <Text size="sm">
            Are you sure you want to terminate query with PID <strong>{pid}</strong>?
          </Text>
        ),
        labels: { confirm: "Terminate", cancel: "Cancel" },
        confirmProps: { color: "red" },
        onConfirm: () => {
          terminateMutation.mutate(
            { pid },
            {
              onSuccess: (res) => {
                notifications.show({
                  title: res.terminated ? "Query terminated" : "Termination sent",
                  message: `Signal sent to PID ${pid}`,
                  color: "green",
                });
              },
              onError: (err) => {
                notifications.show({
                  title: "Failed to terminate",
                  message: err.message,
                  color: "red",
                });
              },
            }
          );
        },
      });
    },
    [terminateMutation]
  );

  const queries = data?.queries ?? [];

  return (
    <Stack gap={0} h="100%">
      <Group gap={4} px="sm" py="xs" style={{ flexShrink: 0 }}>
        <IconActivity size={14} style={{ opacity: 0.5 }} />
        <Text size="xs" fw={500} c="dimmed">
          Active Queries
        </Text>
        {queries.length > 0 && (
          <Badge
            size="xs"
            variant="light"
            color={queries.some((q) => q.state === "active") ? "primary" : "deep"}
          >
            {queries.length}
          </Badge>
        )}
      </Group>
      <ScrollArea style={{ flex: 1 }}>
        {isLoading && (
          <Box ta="center" py="xl">
            <Loader size="sm" />
          </Box>
        )}
        {!isLoading && queries.length === 0 && (
          <Text size="xs" c="dimmed" ta="center" py="xl">
            No active queries
          </Text>
        )}
        {queries.map((q) => (
          <QueryItem
            key={q.pid}
            query={q}
            onTerminate={handleTerminate}
            isTerminating={terminateMutation.isPending}
          />
        ))}
      </ScrollArea>
    </Stack>
  );
}
