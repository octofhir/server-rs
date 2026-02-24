import { Table, Text, Code } from "@/shared/ui";
import type { RunResult } from "../../lib/useViewDefinition";

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
              {col.name}
              <Text size="xs" c="dimmed">
                {col.type}
              </Text>
            </Table.Th>
          ))}
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {result.rows.slice(0, 50).map((row, i) => (
          <Table.Tr key={`row-${i}-${JSON.stringify(row).slice(0, 20)}`}>
            {result.columns.map((col) => (
              <Table.Td key={col.name}>
                <Code size="xs">
                  {JSON.stringify((row as Record<string, unknown>)[col.name])}
                </Code>
              </Table.Td>
            ))}
          </Table.Tr>
        ))}
      </Table.Tbody>
    </Table>
  );
}
