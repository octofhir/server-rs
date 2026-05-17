import { useState } from 'react';
import {
  Box,
  Button,
  Text,
  Alert,
  Code,
  JsonInput,
  Select,
  Loader,
} from '@/shared/ui';
import { ToolWorkspaceLayout } from '@/widgets/tool-workspace';
import { Play, CircleExclamation } from '@gravity-ui/icons';
import { Editor } from '@monaco-editor/react';
import { useMutation } from '@tanstack/react-query';
import { fhirClient } from '@/shared/api/fhirClient';
import { isRecord } from '@/shared/api/guards';
import classes from './CqlConsole.module.css';

interface CqlEvaluationResult {
  resourceType: string;
  parameter: Array<{
    name: string;
    valueString?: string;
    [key: string]: unknown;
  }>;
}

function isCqlEvaluationResult(value: unknown): value is CqlEvaluationResult {
  return (
    isRecord(value) &&
    typeof value.resourceType === 'string' &&
    Array.isArray(value.parameter) &&
    value.parameter.every((parameter) => isRecord(parameter) && typeof parameter.name === 'string')
  );
}

export function CqlConsole() {
  const [expression, setExpression] = useState('1 + 1');
  const [contextType, setContextType] = useState<string | null>(null);
  const [contextValue, setContextValue] = useState('');
  const [parameters, setParameters] = useState('{}');

  const evaluateMutation = useMutation({
    mutationFn: async () => {
      const requestBody: {
        resourceType: string;
        parameter: Array<{ name: string; valueString?: string; valueCode?: string; resource?: unknown }>;
      } = {
        resourceType: 'Parameters',
        parameter: [
          {
            name: 'expression',
            valueString: expression,
          },
        ],
      };

      if (contextType) {
        requestBody.parameter.push({
          name: 'context',
          valueCode: contextType,
        });
      }

      if (contextValue.trim()) {
        try {
          const contextJson = JSON.parse(contextValue);
          requestBody.parameter.push({
            name: 'contextValue',
            resource: contextJson,
          });
        } catch (error) {
          throw new Error(`Invalid context JSON: ${error instanceof Error ? error.message : 'Parse failed'}`);
        }
      }

      if (parameters.trim() && parameters !== '{}') {
        try {
          const paramsJson = JSON.parse(parameters);
          for (const [name, value] of Object.entries(paramsJson)) {
            requestBody.parameter.push({
              name: 'parameter',
              resource: {
                name,
                value,
              },
            });
          }
        } catch (error) {
          throw new Error(`Invalid parameters JSON: ${error instanceof Error ? error.message : 'Parse failed'}`);
        }
      }

      const response = await fhirClient.customRequest({
        method: 'POST',
        url: '/fhir/$cql',
        data: requestBody,
      });
      if (!isCqlEvaluationResult(response.data)) {
        throw new Error('Invalid CQL response');
      }
      return response.data;
    },
  });

  const handleEvaluate = () => {
    evaluateMutation.mutate();
  };

  const extractResult = (data: CqlEvaluationResult | undefined) => {
    if (!data) return null;
    const returnParam = data.parameter?.find((p) => p.name === 'return');
    if (returnParam?.valueString) {
      try {
        return JSON.parse(returnParam.valueString);
      } catch {
        return returnParam.valueString;
      }
    }
    return null;
  };

  const result = extractResult(evaluateMutation.data);

  return (
    <ToolWorkspaceLayout
      title="CQL Console"
      description="Evaluate Clinical Quality Language (CQL) expressions"
      className="page-enter"
    >
      <div className={classes.workspace}>
        <section className={classes.panel}>
          <div className={classes.formStack}>
            <div className={classes.fieldBlock}>
              <Text size="sm" fw={500} className={classes.fieldLabel}>
                CQL Expression
              </Text>
              <div className={classes.editorFrame}>
                <Editor
                  height="200px"
                  defaultLanguage="plaintext"
                  value={expression}
                  onChange={(value) => setExpression(value || '')}
                  theme="vs-dark"
                  options={{
                    minimap: { enabled: false },
                    fontSize: 14,
                    lineNumbers: 'on',
                    scrollBeyondLastLine: false,
                    automaticLayout: true,
                  }}
                />
              </div>
            </div>

            <div className={classes.fieldBlock}>
              <Select
                label="Context Type (optional)"
                placeholder="Select resource type"
                value={contextType}
                onChange={setContextType}
                data={[
                  { value: 'Patient', label: 'Patient' },
                  { value: 'Encounter', label: 'Encounter' },
                  { value: 'Observation', label: 'Observation' },
                  { value: 'Condition', label: 'Condition' },
                ]}
                clearable
              />
            </div>

            <JsonInput
              label="Context Value (optional JSON)"
              placeholder='{"resourceType": "Patient", "id": "123"}'
              value={contextValue}
              onChange={setContextValue}
              minRows={3}
              maxRows={6}
              autosize
            />

            <JsonInput
              label="Parameters (optional JSON)"
              placeholder='{"paramName": 5, "anotherParam": "value"}'
              value={parameters}
              onChange={setParameters}
              minRows={3}
              maxRows={6}
              autosize
            />

            <Button
              leftSection={evaluateMutation.isPending ? <Loader size="xs" /> : <Play size={16} />}
              onClick={handleEvaluate}
              disabled={!expression.trim() || evaluateMutation.isPending}
              fullWidth
            >
              {evaluateMutation.isPending ? 'Evaluating...' : 'Evaluate Expression'}
            </Button>

            <div className={classes.examples}>
              <Text size="xs" fw={600}>
                Example expressions
              </Text>
              <Code block className={classes.exampleCode}>1 + 1</Code>
              <Code block className={classes.exampleCode}>true and false</Code>
              <Code block className={classes.exampleCode}>&apos;Hello&apos; + &apos; &apos; + &apos;World&apos;</Code>
              <Code block className={classes.exampleCode}>5 &gt; 3</Code>
              <Code block className={classes.exampleCode}>&#123;1, 2, 3, 4, 5&#125;</Code>
            </div>
          </div>
        </section>

        <section className={classes.panel}>
          <div className={classes.resultHeader}>
            <Text size="sm" fw={600}>
              Result
            </Text>
          </div>

          {evaluateMutation.isPending && (
            <Box className={classes.emptyState}>
              <Loader size="md" />
              <Text size="sm" c="dimmed">Evaluating...</Text>
            </Box>
          )}

          {evaluateMutation.isError && (
            <Alert
              icon={<CircleExclamation size={16} />}
              title="Evaluation Error"
              color="red"
              radius="md"
            >
              {evaluateMutation.error instanceof Error
                ? evaluateMutation.error.message
                : 'An unknown error occurred'}
            </Alert>
          )}

          {evaluateMutation.isSuccess && (
            <Box className={classes.resultCode}>
              <Code block>
                {JSON.stringify(result, null, 2)}
              </Code>
            </Box>
          )}

          {!evaluateMutation.isPending && !evaluateMutation.isError && !evaluateMutation.isSuccess && (
            <Box className={classes.emptyState}>
              <Text size="sm">
                Enter a CQL expression and click "Evaluate" to see results
              </Text>
            </Box>
          )}
        </section>
      </div>
    </ToolWorkspaceLayout>
  );
}
