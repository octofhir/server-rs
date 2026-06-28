import { Alert, DataPreview, Text } from "@octofhir/ui-kit";
import { CircleAlert as CircleExclamation, Info as CircleInfo } from "lucide-react";
import { type ReactNode, useMemo } from "react";
import type { SqlResponse, SqlValue } from "@/shared/api/types";
import classes from "../DbConsolePage.module.css";
import { JsonCellViewer } from "./JsonCellViewer";
import { cellMatches, inferColumnType } from "./resultExport";

interface ResultsTableProps {
  data: SqlResponse | undefined;
  error: Error | null;
  isPending: boolean;
  /** Case-insensitive substring filter applied across all cells of a row. */
  filter?: string;
  maxHeight?: number | string;
}

function renderCellValue(value: SqlValue): ReactNode {
  if (value === null) {
    return <span className={classes.cellNull}>NULL</span>;
  }
  if (typeof value === "object") {
    return <JsonCellViewer value={value as Record<string, unknown>} />;
  }
  if (typeof value === "boolean") {
    return (
      <span className={value ? classes.cellBoolTrue : classes.cellBoolFalse}>
        {value.toString()}
      </span>
    );
  }
  if (typeof value === "number") {
    return <span className={classes.cellNumber}>{String(value)}</span>;
  }
  return String(value);
}

export function ResultsTable({ data, error, isPending, filter, maxHeight }: ResultsTableProps) {
  const term = filter?.trim().toLowerCase() ?? "";

  const filteredRows = useMemo(() => {
    if (!data) return [];
    if (!term) return data.rows;
    return data.rows.filter((row) => row.some((cell) => cellMatches(cell, term)));
  }, [data, term]);

  const columnTypes = useMemo(() => {
    if (!data) return [];
    return data.columns.map((_col, index) => inferColumnType(data.rows, index));
  }, [data]);

  if (isPending) {
    return (
      <Text c="dimmed" ta="center" py="xl" size="sm">
        Executing query…
      </Text>
    );
  }

  if (error) {
    return (
      <Alert icon={<CircleExclamation size={16} />} color="fire" title="Query Error">
        {error.message}
      </Alert>
    );
  }

  if (!data) {
    return (
      <Text c="dimmed" ta="center" py="xl" size="sm">
        Run a query to see results
      </Text>
    );
  }

  if (data.rowCount === 0) {
    return (
      <Alert icon={<CircleInfo size={16} />} color="primary">
        Query executed successfully. No rows returned.
      </Alert>
    );
  }

  if (term && filteredRows.length === 0) {
    return (
      <Text c="dimmed" ta="center" py="xl" size="sm">
        No rows match “{filter}”
      </Text>
    );
  }

  return (
    <DataPreview
      className={classes.resultGrid}
      columns={data.columns.map((column, columnIndex) => ({
        id: `${column}-${columnIndex}`,
        label: (
          <span className={classes.colHead}>
            <span className={classes.colName}>{column}</span>
            <span className={classes.colType}>{columnTypes[columnIndex]}</span>
          </span>
        ),
        width: 180,
      }))}
      rows={filteredRows.map((row) =>
        Object.fromEntries(
          data.columns.map((column, columnIndex) => [
            `${column}-${columnIndex}`,
            renderCellValue(row[columnIndex]),
          ])
        )
      )}
      getRowKey={(_row, rowIndex) => `${rowIndex}`}
      maxHeight={maxHeight ?? "100%"}
    />
  );
}
