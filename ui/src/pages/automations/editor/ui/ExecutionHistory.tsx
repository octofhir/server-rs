import {
  Stack,
  Table,
  Text,
  Badge,
  Loader,
  Center,
  ScrollArea,
  Tooltip,
  Code,
  Paper,
  Group,
  ActionIcon,
} from "@/shared/ui";
import { useState } from "react";
import { IconChevronDown, IconChevronRight } from "@tabler/icons-react";
import { useAutomationLogs } from "../../lib/useAutomations";
import type { AutomationExecution, AutomationExecutionStatus } from "@/shared/api/types";

interface ExecutionHistoryProps {
  automationId: string;
}

const statusConfig: Record<AutomationExecutionStatus, { color: string; label: string }> = {
  running: { color: "blue", label: "Running" },
  completed: { color: "green", label: "Completed" },
  failed: { color: "red", label: "Failed" },
};

function ExecutionRow({ execution }: { execution: AutomationExecution }) {
  const [expanded, setExpanded] = useState(false);
  const config = statusConfig[execution.status] || statusConfig.failed;

  const formatDate = (dateString: string) => {
    const date = new Date(dateString);
    return date.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  };

  const formatDuration = (ms?: number) => {
    if (ms === undefined) return "-";
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(2)}s`;
  };

  return (
    <>
      <Table.Tr
        style={{ cursor: "pointer" }}
        onClick={() => setExpanded(!expanded)}
      >
        <Table.Td>
          <Group gap={4}>
            <ActionIcon variant="subtle" size="xs">
              {expanded ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
            </ActionIcon>
            <Text size="sm">{formatDate(execution.started_at)}</Text>
          </Group>
        </Table.Td>
        <Table.Td>
          <Badge color={config.color} variant="light" size="sm">
            {config.label}
          </Badge>
        </Table.Td>
        <Table.Td>
          <Text size="sm" c="dimmed">
            {formatDuration(execution.duration_ms)}
          </Text>
        </Table.Td>
        <Table.Td>
          <Tooltip label={execution.trigger_id || "Manual"}>
            <Badge size="xs" variant="outline">
              {execution.trigger_id ? "Trigger" : "Manual"}
            </Badge>
          </Tooltip>
        </Table.Td>
      </Table.Tr>
      {expanded && (
        <Table.Tr>
          <Table.Td colSpan={4} p={0}>
            <Paper p="md" bg="var(--mantine-color-gray-light)">
              <Stack gap="sm">
                {execution.error && (
                  <div>
                    <Text size="xs" fw={500} c="red">Error:</Text>
                    <Code block color="red" mt={4}>
                      {execution.error}
                    </Code>
                  </div>
                )}
                {execution.input && (
                  <div>
                    <Text size="xs" fw={500}>Input:</Text>
                    <Code block mt={4}>
                      {JSON.stringify(execution.input, null, 2)}
                    </Code>
                  </div>
                )}
                {execution.output !== undefined && (
                  <div>
                    <Text size="xs" fw={500}>Output:</Text>
                    <Code block mt={4}>
                      {JSON.stringify(execution.output, null, 2)}
                    </Code>
                  </div>
                )}
                {execution.logs && execution.logs.length > 0 && (
                  <div>
                    <Text size="xs" fw={500}>Execution Logs:</Text>
                    <Stack gap={4} mt={4}>
                      {execution.logs.map((log, index) => (
                        <Paper key={`${index}-${log.timestamp ?? log.message.slice(0, 20)}`} p="xs" withBorder>
                          <Group gap="xs" wrap="nowrap" align="flex-start">
                            <Badge
                              size="xs"
                              color={
                                log.level === "error" ? "red" :
                                log.level === "warn" ? "yellow" :
                                log.level === "debug" ? "gray" : "blue"
                              }
                              style={{ flexShrink: 0 }}
                            >
                              {log.level}
                            </Badge>
                            <Stack gap={2} style={{ flex: 1, minWidth: 0 }}>
                              <Text size="xs" style={{ fontFamily: "monospace" }}>
                                {log.message}
                              </Text>
                              {log.data !== undefined && log.data !== null && (
                                <Code block size="xs">
                                  {typeof log.data === "string" ? log.data : JSON.stringify(log.data, null, 2)}
                                </Code>
                              )}
                            </Stack>
                            {log.timestamp && (
                              <Text size="xs" c="dimmed" style={{ flexShrink: 0 }}>
                                {new Date(log.timestamp).toLocaleTimeString()}
                              </Text>
                            )}
                          </Group>
                        </Paper>
                      ))}
                    </Stack>
                  </div>
                )}
              </Stack>
            </Paper>
          </Table.Td>
        </Table.Tr>
      )}
    </>
  );
}

export function ExecutionHistory({ automationId }: ExecutionHistoryProps) {
  const { data: executions, isLoading, error } = useAutomationLogs(automationId);

  if (isLoading) {
    return (
      <Center h={200}>
        <Loader size="sm" />
      </Center>
    );
  }

  if (error) {
    return (
      <Center h={200}>
        <Text c="red" size="sm">
          Failed to load execution history
        </Text>
      </Center>
    );
  }

  if (!executions || executions.length === 0) {
    return (
      <Center h={200}>
        <Text c="dimmed" size="sm">
          No execution history yet. Run the automation to see results here.
        </Text>
      </Center>
    );
  }

  return (
    <ScrollArea h="100%">
      <Table striped>
        <Table.Thead>
          <Table.Tr>
            <Table.Th>Time</Table.Th>
            <Table.Th>Status</Table.Th>
            <Table.Th>Duration</Table.Th>
            <Table.Th>Source</Table.Th>
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {executions.map((execution) => (
            <ExecutionRow key={execution.id} execution={execution} />
          ))}
        </Table.Tbody>
      </Table>
    </ScrollArea>
  );
}
