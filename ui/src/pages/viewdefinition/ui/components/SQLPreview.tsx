import { Code, Text, Loader } from "@/shared/ui";
import type { SqlResult } from "../../lib/useViewDefinition";
import classes from "./SQLPreview.module.css";

interface SQLPreviewProps {
  result: SqlResult | null;
  isLoading: boolean;
  error: Error | null;
}

export function SQLPreview({ result, isLoading, error }: SQLPreviewProps) {
  if (isLoading) {
    return (
      <div className={classes.state}>
        <Loader size="sm" />
        <Text size="sm" c="dimmed">
          Generating SQL...
        </Text>
      </div>
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
    <Code block className={classes.sqlCode}>
      {result.sql}
    </Code>
  );
}
