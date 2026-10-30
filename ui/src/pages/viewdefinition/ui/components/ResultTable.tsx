import { Table, Text } from "@octofhir/ui-kit";
import { isRecord } from "@/shared/api/guards";
import type { RunResult } from "../../lib/useViewDefinition";
import classes from "./ResultTable.module.css";

interface ResultTableProps {
  result: RunResult;
}

export function ResultTable({ result }: ResultTableProps) {
  if (result.rows.length === 0) {
    return (
      <Text c="dimmed" size="sm" ta="center" py="md">
        No results
      </Text>
    );
  }

  return (
    <Table striped highlightOnHover withTableBorder withColumnBorders>
      <Table.Thead>
        <Table.Tr>
          {result.columns.map((col) => (
            <Table.Th key={col.name}>
              {col.name} <span className={classes.type}>{col.type}</span>
            </Table.Th>
          ))}
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {result.rows.slice(0, 50).map((row, i) => (
          <Table.Tr key={`row-${i}-${JSON.stringify(row).slice(0, 20)}`}>
            {result.columns.map((col) => (
              <Table.Td key={col.name}>
                <CellValue value={isRecord(row) ? row[col.name] : undefined} />
              </Table.Td>
            ))}
          </Table.Tr>
        ))}
      </Table.Tbody>
    </Table>
  );
}

/** Scalars read as plain values; only structured values fall back to JSON. */
function CellValue({ value }: { value: unknown }) {
  if (value === null || value === undefined) return <span className={classes.null}>null</span>;
  if (typeof value === "object") return <span className={classes.json}>{JSON.stringify(value)}</span>;
  return <span className={typeof value === "string" ? undefined : classes.scalar}>{String(value)}</span>;
}
