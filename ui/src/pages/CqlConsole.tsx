import { useState } from 'react';
import {
  Box,
  Title,
  Paper,
  Stack,
  Button,
  Group,
  Text,
  Alert,
  Code,
  JsonInput,
  Select,
  Loader,
  Grid,
} from '@/shared/ui';
import { IconPlayerPlay, IconAlertCircle, IconCheck } from '@tabler/icons-react';
import { Editor } from '@monaco-editor/react';
import { useMutation } from '@tanstack/react-query';
import { fhirClient } from '@/shared/api/fhirClient';

interface CqlEvaluationResult {
  resourceType: string;
  parameter: Array<{
    name: string;
    valueString?: string;
    [key: string]: unknown;
  }>;
}

export function CqlConsole() {
  const [expression, setExpression] = useState('1 + 1');
  const [contextType, setContextType] = useState<string | null>(null);
  const [contextValue, setContextValue] = useState('');
  const [parameters, setParameters] = useState('{}');

  const evaluateMutation = useMutation({
    mutationFn: async () => {
      console.log('=== CQL Evaluation Starting ===');
      console.log('Expression:', expression);

      // Build Parameters resource for $cql operation
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

      console.log('Request body:', requestBody);

      // Add context if provided
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
        } catch (e) {
          throw new Error(`Invalid context JSON: ${e}`);
        }
      }

      // Add parameters if provided
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
        } catch (e) {
          throw new Error(`Invalid parameters JSON: ${e}`);
        }
      }

      // Call $cql operation
      console.log('Calling API endpoint: POST /fhir/$cql');
      const response = await fhirClient.customRequest<CqlEvaluationResult>({
        method: 'POST',
        url: '/fhir/$cql',
        data: requestBody,
      });
      console.log('Response:', response);
      return response.data;
    },
  });

  const handleEvaluate = () => {
    console.log('=== Button clicked ===');
    console.log('Current expression:', expression);
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
    <Box
      className="page-enter"
      style={{
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      <Box p="xl" style={{ overflowY: 'auto', flex: 1 }}>
        <Stack gap="lg" style={{ maxWidth: '100%', height: '100%' }}>
        <Box>
          <Title order={2}>CQL Console</Title>
          <Text c="dimmed" size="sm">
            Evaluate Clinical Quality Language (CQL) expressions
          </Text>
        </Box>

        <Grid gutter="lg" style={{ flex: 1 }}>
          <Grid.Col span={6}>
            <Paper p="md" radius="lg" withBorder style={{ height: '100%' }}>
          <Stack gap="md">
            <div>
              <Text size="sm" fw={500} mb="xs">
                CQL Expression
              </Text>
              <Paper
                withBorder
                radius="md"
                style={{
                  overflow: 'hidden',
                  border: '1px solid var(--octo-border-subtle)',
                }}
              >
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
              </Paper>
            </div>

            <Group grow align="flex-start">
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
            </Group>

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
              leftSection={evaluateMutation.isPending ? <Loader size="xs" /> : <IconPlayerPlay size={16} />}
              onClick={handleEvaluate}
              disabled={!expression.trim() || evaluateMutation.isPending}
              fullWidth
            >
              {evaluateMutation.isPending ? 'Evaluating...' : 'Evaluate Expression'}
            </Button>

            <Paper p="xs" radius="lg" withBorder>
              <Stack gap="xs">
                <Text size="xs" fw={600}>
                  Example Expressions:
                </Text>
                <Code block style={{ fontSize: '11px' }}>1 + 1</Code>
                <Code block style={{ fontSize: '11px' }}>true and false</Code>
                <Code block style={{ fontSize: '11px' }}>&apos;Hello&apos; + &apos; &apos; + &apos;World&apos;</Code>
                <Code block style={{ fontSize: '11px' }}>5 &gt; 3</Code>
                <Code block style={{ fontSize: '11px' }}>&#123;1, 2, 3, 4, 5&#125;</Code>
              </Stack>
            </Paper>
          </Stack>
        </Paper>
          </Grid.Col>

          <Grid.Col span={6}>
            <Paper p="md" radius="lg" withBorder style={{ height: '100%' }}>
              <Stack gap="md" style={{ height: '100%' }}>
                <Text size="sm" fw={600}>
                  Result
                </Text>

        {evaluateMutation.isPending && (
          <Box style={{ textAlign: 'center', padding: '2rem' }}>
            <Loader size="md" />
            <Text size="sm" c="dimmed" mt="md">Evaluating...</Text>
          </Box>
        )}

        {evaluateMutation.isError && (
          <Alert
            icon={<IconAlertCircle size={16} />}
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
          <Box>
            <Code block style={{ maxHeight: '600px', overflow: 'auto' }}>
              {JSON.stringify(result, null, 2)}
            </Code>
          </Box>
        )}

        {!evaluateMutation.isPending && !evaluateMutation.isError && !evaluateMutation.isSuccess && (
          <Box style={{ textAlign: 'center', padding: '3rem', color: 'var(--mantine-color-dimmed)' }}>
            <Text size="sm">
              Enter a CQL expression and click "Evaluate" to see results
            </Text>
          </Box>
        )}
              </Stack>
            </Paper>
          </Grid.Col>
        </Grid>
      </Stack>
      </Box>
    </Box>
  );
}
