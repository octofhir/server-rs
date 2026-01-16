import { useState } from "react";
import {
  Stack,
  Group,
  Paper,
  Text,
  Button,
  Select,
  Code,
  ScrollArea,
  Badge,
  Divider,
  Collapse,
  ActionIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconPlayerPlay, IconTrash, IconChevronDown, IconChevronRight } from "@tabler/icons-react";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { useTestAutomation } from "../../lib/useAutomations";
import type { ExecuteAutomationResponse, AutomationLogEntry } from "@/shared/api/types";

interface PlaygroundPanelProps {
  automationId: string;
  /** Current source code from editor (used to test without saving) */
  sourceCode: string;
}

const eventTemplates = [
  {
    value: "patient-created",
    label: "Patient Created",
    input: {
      resource: {
        resourceType: "Patient",
        id: "example-patient-id",
        active: true,
        name: [{ use: "official", family: "Smith", given: ["John"] }],
        gender: "male",
        birthDate: "1990-01-15",
      },
      event_type: "created",
    },
  },
  {
    value: "observation-created",
    label: "Observation Created",
    input: {
      resource: {
        resourceType: "Observation",
        id: "example-obs-id",
        status: "final",
        code: {
          coding: [{ system: "http://loinc.org", code: "29463-7", display: "Body Weight" }],
        },
        subject: { reference: "Patient/example-patient-id" },
        valueQuantity: { value: 70, unit: "kg", system: "http://unitsofmeasure.org", code: "kg" },
      },
      event_type: "created",
    },
  },
  {
    value: "task-updated",
    label: "Task Updated",
    input: {
      resource: {
        resourceType: "Task",
        id: "example-task-id",
        status: "completed",
        intent: "order",
        description: "Follow-up appointment",
      },
      event_type: "updated",
    },
  },
  {
    value: "manual",
    label: "Manual (empty)",
    input: {
      event_type: "manual",
    },
  },
];

function LogLevelBadge({ level }: { level: AutomationLogEntry["level"] }) {
  const colors: Record<string, string> = {
    log: "blue",
    info: "blue",
    debug: "gray",
    warn: "yellow",
    error: "red",
  };
  return (
    <Badge size="xs" color={colors[level] || "gray"} variant="light">
      {level.toUpperCase()}
    </Badge>
  );
}

export function PlaygroundPanel({ automationId, sourceCode }: PlaygroundPanelProps) {
  const [inputJson, setInputJson] = useState<string>(
    JSON.stringify(eventTemplates[0].input, null, 2),
  );
  const [result, setResult] = useState<ExecuteAutomationResponse | null>(null);
  const [outputExpanded, setOutputExpanded] = useState(true);
  const [logsExpanded, setLogsExpanded] = useState(true);

  const testMutation = useTestAutomation(automationId);

  const handleTemplateChange = (value: string | null) => {
    const template = eventTemplates.find((t) => t.value === value);
    if (template) {
      setInputJson(JSON.stringify(template.input, null, 2));
    }
  };

  const handleExecute = async () => {
    try {
      const input = JSON.parse(inputJson);
      // Use test endpoint with current source code from editor
      const response = await testMutation.mutateAsync({
        source_code: sourceCode,
        resource: input.resource,
        event_type: input.event_type,
      });
      setResult(response);

      if (response.success) {
        notifications.show({
          title: "Execution Completed",
          message: `Completed in ${response.duration_ms}ms`,
          color: "green",
        });
      } else {
        notifications.show({
          title: "Execution Failed",
          message: response.error || "Unknown error",
          color: "red",
        });
      }
    } catch (error) {
      if (error instanceof SyntaxError) {
        notifications.show({
          title: "Invalid JSON",
          message: "Please fix the input JSON syntax",
          color: "red",
        });
      } else {
        notifications.show({
          title: "Execution Error",
          message: error instanceof Error ? error.message : "Unknown error",
          color: "red",
        });
      }
    }
  };

  const handleClear = () => {
    setResult(null);
  };

  return (
    <Stack gap="md" h="100%">
      {/* Input section */}
      <Stack gap="xs">
        <Group justify="space-between">
          <Text fw={500} size="sm">Input Context</Text>
          <Select
            size="xs"
            placeholder="Load template"
            data={eventTemplates.map((t) => ({ value: t.value, label: t.label }))}
            onChange={handleTemplateChange}
            clearable
            w={200}
          />
        </Group>
        <Paper withBorder radius="md" style={{ overflow: "hidden" }}>
          <JsonEditor
            value={inputJson}
            onChange={(value) => setInputJson(value || "")}
            height={180}
          />
        </Paper>
        <Group>
          <Button
            leftSection={<IconPlayerPlay size={16} />}
            onClick={handleExecute}
            loading={testMutation.isPending}
          >
            Test
          </Button>
          {result && (
            <Button variant="subtle" color="gray" leftSection={<IconTrash size={16} />} onClick={handleClear}>
              Clear Results
            </Button>
          )}
        </Group>
      </Stack>

      <Divider />

      {/* Results section */}
      {result && (
        <Stack gap="sm" style={{ flex: 1, minHeight: 0 }}>
          {/* Status */}
          <Group gap="md">
            <Badge color={result.success ? "green" : "red"} size="lg">
              {result.success ? "Success" : "Failed"}
            </Badge>
            {result.duration_ms !== undefined && (
              <Text size="sm" c="dimmed">
                Duration: {result.duration_ms}ms
              </Text>
            )}
          </Group>

          {/* Error */}
          {result.error && (
            <Paper withBorder p="sm" radius="md" bg="var(--mantine-color-red-light)">
              <Text size="sm" c="red" fw={500}>Error:</Text>
              <Code block color="red" mt="xs">
                {result.error}
              </Code>
            </Paper>
          )}

          {/* Output */}
          {result.output !== undefined && (
            <Paper withBorder radius="md" p={0}>
              <Group
                p="xs"
                style={{ cursor: "pointer" }}
                onClick={() => setOutputExpanded(!outputExpanded)}
              >
                <ActionIcon variant="subtle" size="sm">
                  {outputExpanded ? <IconChevronDown size={16} /> : <IconChevronRight size={16} />}
                </ActionIcon>
                <Text size="sm" fw={500}>Output</Text>
              </Group>
              <Collapse in={outputExpanded}>
                <Divider />
                <ScrollArea h={120} p="xs">
                  <Code block>
                    {JSON.stringify(result.output, null, 2)}
                  </Code>
                </ScrollArea>
              </Collapse>
            </Paper>
          )}

          {/* Execution Logs */}
          {result.logs && result.logs.length > 0 && (
            <Paper withBorder radius="md" p={0}>
              <Group
                p="xs"
                style={{ cursor: "pointer" }}
                onClick={() => setLogsExpanded(!logsExpanded)}
              >
                <ActionIcon variant="subtle" size="sm">
                  {logsExpanded ? <IconChevronDown size={16} /> : <IconChevronRight size={16} />}
                </ActionIcon>
                <Text size="sm" fw={500}>Logs ({result.logs.length})</Text>
              </Group>
              <Collapse in={logsExpanded}>
                <Divider />
                <ScrollArea h={180}>
                  <Stack gap={4} p="xs">
                    {result.logs.map((log, index) => (
                      <Paper key={`${index}-${log.message.slice(0, 20)}`} p="xs" withBorder>
                        <Group gap="xs" wrap="nowrap" align="flex-start">
                          <LogLevelBadge level={log.level} />
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
                </ScrollArea>
              </Collapse>
            </Paper>
          )}
        </Stack>
      )}

      {!result && (
        <Paper withBorder p="xl" radius="md" ta="center">
          <Text c="dimmed" size="sm">
            Click "Execute" to run the automation with the input context above.
          </Text>
          <Text c="dimmed" size="xs" mt="xs">
            Tip: Press Ctrl+Enter in the editor to execute
          </Text>
        </Paper>
      )}
    </Stack>
  );
}
