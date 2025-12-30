import { Stack, Tabs, Text, ScrollArea } from "@mantine/core";
import { SQLPreview } from "./SQLPreview";
import { ResultTable } from "./ResultTable";
import type { RunResult, SqlResult } from "../../lib/useViewDefinition";

interface PreviewPanelProps {
  sqlResult: SqlResult | null;
  sqlLoading: boolean;
  sqlError: Error | null;
  runResult: RunResult | null;
  onGenerateSql: () => void;
}

export function PreviewPanel({ sqlResult, sqlLoading, sqlError, runResult, onGenerateSql }: PreviewPanelProps) {
  return (
    <Stack gap="md" style={{ flex: 1 }}>
      <Tabs
        defaultValue="sql"
        style={{ flex: 1, display: "flex", flexDirection: "column" }}
        onChange={(value) => {
          if (value === "sql") {
            onGenerateSql();
          }
        }}
      >
        <Tabs.List>
          <Tabs.Tab value="sql">SQL</Tabs.Tab>
          <Tabs.Tab value="results">Results{runResult && ` (${runResult.rowCount})`}</Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="sql" pt="md" style={{ flex: 1, overflow: "auto" }}>
          <SQLPreview result={sqlResult} isLoading={sqlLoading} error={sqlError} />
        </Tabs.Panel>

        <Tabs.Panel value="results" pt="md" style={{ flex: 1 }}>
          {runResult ? (
            <ScrollArea style={{ maxHeight: "calc(100vh - 300px)" }}>
              <ResultTable result={runResult} />
            </ScrollArea>
          ) : (
            <Text c="dimmed" size="sm" ta="center" py="md">
              Click "Run" to see results
            </Text>
          )}
        </Tabs.Panel>
      </Tabs>
    </Stack>
  );
}
