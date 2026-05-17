import { useState } from "react";
import {
  Text,
  Button,
  Select,
  Code,
  Badge,
  Divider,
  Collapse,
  ActionIcon,
} from "@/shared/ui";
import { notifications } from "@octofhir/ui-kit";
import { IconPlayerPlay, IconTrash, IconChevronDown, IconChevronRight } from "@octofhir/ui-kit";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { useTestAutomation } from "../../lib/useAutomations";
import type { ExecuteAutomationResponse, AutomationLogEntry } from "@/shared/api/types";
import { isRecord } from "@/shared/api/guards";
import classes from "./PlaygroundPanel.module.css";

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
      const input: unknown = JSON.parse(inputJson);
      if (!isRecord(input)) {
        throw new SyntaxError("Input JSON must be an object");
      }
      // Use test endpoint with current source code from editor
      const response = await testMutation.mutateAsync({
        source_code: sourceCode,
        resource: input.resource,
        event_type: typeof input.event_type === "string" ? input.event_type : undefined,
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
    <div className={classes.root}>
      {/* Input section */}
      <div className={classes.inputSection}>
        <div className={classes.inputHeader}>
          <Text fw={500} size="sm">Input Context</Text>
          <Select
            size="xs"
            placeholder="Load template"
            data={eventTemplates.map((t) => ({ value: t.value, label: t.label }))}
            onChange={handleTemplateChange}
            clearable
            className={classes.templateSelect}
          />
        </div>
        <div className={classes.editorFrame}>
          <JsonEditor
            value={inputJson}
            onChange={(value) => setInputJson(value || "")}
            height={180}
          />
        </div>
        <div className={classes.inputActions}>
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
        </div>
      </div>

      <Divider />

      {/* Results section */}
      {result && (
        <div className={classes.results}>
          {/* Status */}
          <div className={classes.statusRow}>
            <Badge color={result.success ? "green" : "red"} size="lg">
              {result.success ? "Success" : "Failed"}
            </Badge>
            {result.duration_ms !== undefined && (
              <Text size="sm" c="dimmed">
                Duration: {result.duration_ms}ms
              </Text>
            )}
          </div>

          {/* Error */}
          {result.error && (
            <div className={classes.errorCard}>
              <Text size="sm" c="red" fw={500}>Error:</Text>
              <Code block color="red" mt="xs">
                {result.error}
              </Code>
            </div>
          )}

          {/* Output */}
          {result.output !== undefined && (
            <div className={classes.resultCard}>
              <div
                className={classes.collapseHeader}
                onClick={() => setOutputExpanded(!outputExpanded)}
              >
                <ActionIcon variant="subtle" size="sm">
                  {outputExpanded ? <IconChevronDown size={16} /> : <IconChevronRight size={16} />}
                </ActionIcon>
                <Text size="sm" fw={500}>Output</Text>
              </div>
              <Collapse in={outputExpanded}>
                <Divider />
                <div className={classes.outputScroll}>
                  <Code block>
                    {JSON.stringify(result.output, null, 2)}
                  </Code>
                </div>
              </Collapse>
            </div>
          )}

          {/* Execution Logs */}
          {result.logs && result.logs.length > 0 && (
            <div className={classes.resultCard}>
              <div
                className={classes.collapseHeader}
                onClick={() => setLogsExpanded(!logsExpanded)}
              >
                <ActionIcon variant="subtle" size="sm">
                  {logsExpanded ? <IconChevronDown size={16} /> : <IconChevronRight size={16} />}
                </ActionIcon>
                <Text size="sm" fw={500}>Logs ({result.logs.length})</Text>
              </div>
              <Collapse in={logsExpanded}>
                <Divider />
                <div className={classes.logsScroll}>
                  <div className={classes.logsList}>
                    {result.logs.map((log, index) => (
                      <div key={`${index}-${log.message.slice(0, 20)}`} className={classes.logItem}>
                        <div className={classes.logRow}>
                          <LogLevelBadge level={log.level} />
                          <div className={classes.logBody}>
                            <Text size="xs" className={classes.logMessage}>
                              {log.message}
                            </Text>
                            {log.data !== undefined && log.data !== null && (
                              <Code block size="xs">
                                {typeof log.data === "string" ? log.data : JSON.stringify(log.data, null, 2)}
                              </Code>
                            )}
                          </div>
                          {log.timestamp && (
                            <Text size="xs" c="dimmed" className={classes.logTime}>
                              {new Date(log.timestamp).toLocaleTimeString()}
                            </Text>
                          )}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </Collapse>
            </div>
          )}
        </div>
      )}

      {!result && (
        <div className={classes.emptyState}>
          <Text c="dimmed" size="sm">
            Click "Execute" to run the automation with the input context above.
          </Text>
          <Text c="dimmed" size="xs" mt="xs">
            Tip: Press Ctrl+Enter in the editor to execute
          </Text>
        </div>
      )}
    </div>
  );
}
