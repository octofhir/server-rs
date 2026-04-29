import { Flex, Tabs, Text, Box } from "@/shared/ui";
import { SQLPreview } from "./SQLPreview";
import { ResultTable } from "./ResultTable";
import type { RunResult, SqlResult } from "../../lib/useViewDefinition";

interface PreviewPanelProps {
  runResult: RunResult | null;
  sqlResult: SqlResult | null;
  onRefreshSql: () => void;
  isLoading: boolean;
}

export function PreviewPanel({ runResult, sqlResult, onRefreshSql, isLoading }: PreviewPanelProps) {
  return (
    <Flex direction="column" gap="4" style={{ flex: 1, height: "100%" }}>
      <Tabs
        defaultValue="sql"
        style={{ flex: 1, display: "flex", flexDirection: "column" }}
        onUpdate={(value) => {
          if (value === "sql") {
            onRefreshSql();
          }
        }}
      >
        <Tabs.List>
          <Tabs.Tab value="sql">SQL Preview</Tabs.Tab>
          <Tabs.Tab value="results">Results{runResult && ` (${runResult.rowCount})`}</Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="sql" style={{ flex: 1, overflow: "auto", paddingTop: 16 }}>
          <SQLPreview result={sqlResult} isLoading={isLoading} error={null} />
        </Tabs.Panel>

        <Tabs.Panel value="results" style={{ flex: 1, overflow: "hidden", paddingTop: 16 }}>
          {runResult ? (
            <Box style={{ height: "100%", overflow: "auto" }}>
              <ResultTable result={runResult} />
            </Box>
          ) : (
            <Flex grow alignItems="center" justifyContent="center" py="10">
              <Text color="secondary" variant="body-1">
                Click "Run" to see preview results
              </Text>
            </Flex>
          )}
        </Tabs.Panel>
      </Tabs>
    </Flex>
  );
}
