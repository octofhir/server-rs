import { Code, Text, Loader, Stack } from "@/shared/ui";
import type { SqlResult } from "../../lib/useViewDefinition";

interface SQLPreviewProps {
  result: SqlResult | null;
  isLoading: boolean;
  error: Error | null;
}

export function SQLPreview({ result, isLoading, error }: SQLPreviewProps) {
  if (isLoading) {
    return (
      <Stack align="center" justify="center" h={200}>
        <Loader size="sm" />
        <Text size="sm" c="dimmed">
          Generating SQL...
        </Text>
      </Stack>
    );
  }

  if (error) {
    return (
      <Text c="red" size="sm" p="md">
        {error.message}
      </Text>
    );
  }

  if (!result || !result.sql) {
    return (
      <Text c="dimmed" size="sm" ta="center" py="md">
        Define columns to see generated SQL
      </Text>
    );
  }

  return (
    <Code block style={{ maxHeight: "calc(100vh - 300px)", overflow: "auto" }}>
      {result.sql}
    </Code>
  );
}
