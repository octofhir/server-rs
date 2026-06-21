import { useState } from 'react';
import {
  Button,
  Text,
  Alert,
  Code,
  JsonInput,
  Select,
  Loader,
  Resizable,
  PageContainer,
  ScrollableContent,
  PageHeader,
} from '@octofhir/ui-kit';
import { Play } from "lucide-react";
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
    <PageContainer className="page-enter">
      <div style={{ padding: '14px 20px', borderBottom: '1px solid var(--g-color-line-generic)' }}>
        <PageHeader
          title="CQL Console"
          description="Evaluate Clinical Quality Language (CQL) expressions"
        />
      </div>

      <div style={{ flex: 1, minHeight: 0, width: '100%' }}>
        <Resizable.Group orientation="horizontal">
          <Resizable.Pane defaultSize={45} minSize={30}>
            <ScrollableContent>
              <div className={classes.formStack}>
                <div className={classes.fieldBlock}>
                  <Text variant="body-2" style={{ fontWeight: 500, marginBottom: 4 }}>
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
                  view="action"
                  size="lg"
                  onClick={handleEvaluate}
                  disabled={!expression.trim() || evaluateMutation.isPending}
                  style={{ width: '100%', marginTop: 8 }}
                >
                  {evaluateMutation.isPending ? (
                    <>
                      <Loader size="sm" style={{ marginRight: 8 }} />
                      Evaluating...
                    </>
                  ) : (
                    <>
                      <Play size={16} style={{ marginRight: 8 }} />
                      Evaluate Expression
                    </>
                  )}
                </Button>

                <div className={classes.examples}>
                  <Text variant="caption-2" style={{ fontWeight: 600, textTransform: 'uppercase', color: 'var(--g-color-text-secondary)' }}>
                    Example expressions
                  </Text>
                  <Code className={classes.exampleCode}>1 + 1</Code>
                  <Code className={classes.exampleCode}>true and false</Code>
                  <Code className={classes.exampleCode}>&apos;Hello&apos; + &apos; &apos; + &apos;World&apos;</Code>
                  <Code className={classes.exampleCode}>5 &gt; 3</Code>
                  <Code className={classes.exampleCode}>&#123;1, 2, 3, 4, 5&#125;</Code>
                </div>
              </div>
            </ScrollableContent>
          </Resizable.Pane>

          <Resizable.Handle />

          <Resizable.Pane defaultSize={55} minSize={30}>
            <ScrollableContent style={{ borderLeft: '1px solid var(--g-color-line-generic)' }}>
              <div className={classes.resultHeader}>
                <Text variant="subheader-2">Result</Text>
              </div>

              {evaluateMutation.isPending && (
                <div className={classes.emptyState}>
                  <Loader size="lg" />
                  <Text color="secondary">Evaluating...</Text>
                </div>
              )}

              {evaluateMutation.isError && (
                <Alert
                  view="filled"
                  theme="danger"
                  title="Evaluation Error"
                  style={{ borderRadius: 8 }}
                >
                  {evaluateMutation.error instanceof Error
                    ? evaluateMutation.error.message
                    : 'An unknown error occurred'}
                </Alert>
              )}

              {evaluateMutation.isSuccess && (
                <div className={classes.resultCode}>
                  <Code style={{ display: 'block', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                    {JSON.stringify(result, null, 2)}
                  </Code>
                </div>
              )}

              {!evaluateMutation.isPending && !evaluateMutation.isError && !evaluateMutation.isSuccess && (
                <div className={classes.emptyState}>
                  <Text color="secondary">
                    Enter a CQL expression and click "Evaluate" to see results
                  </Text>
                </div>
              )}
            </ScrollableContent>
          </Resizable.Pane>
        </Resizable.Group>
      </div>
    </PageContainer>
  );
}
