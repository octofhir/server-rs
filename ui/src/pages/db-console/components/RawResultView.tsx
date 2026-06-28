import { Text } from "@octofhir/ui-kit";
import { useMemo } from "react";
import type { SqlResponse } from "@/shared/api/types";
import classes from "../DbConsolePage.module.css";
import { highlightJson } from "./jsonHighlight";
import { cellMatches, resultToObjects } from "./resultExport";

interface RawResultViewProps {
  data: SqlResponse | undefined;
  error: Error | null;
  isPending: boolean;
  filter?: string;
}

export function RawResultView({ data, error, isPending, filter }: RawResultViewProps) {
  const term = filter?.trim().toLowerCase() ?? "";

  const json = useMemo(() => {
    if (!data) return "";
    const objects = resultToObjects(data);
    const visible = term
      ? objects.filter((obj) => Object.values(obj).some((cell) => cellMatches(cell, term)))
      : objects;
    return JSON.stringify(visible, null, 2);
  }, [data, term]);

  if (isPending) {
    return (
      <Text c="dimmed" ta="center" py="xl" size="sm">
        Executing query…
      </Text>
    );
  }

  if (error) {
    return (
      <Text c="var(--octo-accent-fire)" ff="monospace" size="xs" className={classes.preWrap}>
        {error.message}
      </Text>
    );
  }

  if (!data || data.rowCount === 0) {
    return (
      <Text c="dimmed" ta="center" py="xl" size="sm">
        No rows to show
      </Text>
    );
  }

  return (
    <pre className={classes.rawJson}>
      <code>{highlightJson(json)}</code>
    </pre>
  );
}
