import { Text } from "@octofhir/ui-kit";
import { useMemo } from "react";
import type { SqlResponse } from "@/shared/api/types";
import { PlanViewer } from "@/widgets/plan-viewer";

interface ExplainPaneProps {
  data: SqlResponse | undefined;
  error: Error | null;
  isPending: boolean;
}

export function ExplainPane({ data, error, isPending }: ExplainPaneProps) {
  // EXPLAIN (ANALYZE, FORMAT JSON) returns a single cell holding the plan document.
  const plan = useMemo(() => {
    if (!data || data.rowCount === 0) return null;
    return data.rows[0]?.[0] ?? null;
  }, [data]);

  if (isPending) {
    return (
      <Text c="dimmed" ta="center" py="xl" size="sm">
        Running EXPLAIN ANALYZE…
      </Text>
    );
  }

  if (error) {
    return (
      <Text c="dimmed" ta="center" py="xl" size="sm">
        EXPLAIN not available for this query
      </Text>
    );
  }

  if (!plan) {
    return (
      <Text c="dimmed" ta="center" py="xl" size="sm">
        No execution plan available
      </Text>
    );
  }

  return <PlanViewer plan={plan} analyzed />;
}
