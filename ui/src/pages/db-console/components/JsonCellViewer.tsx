import { ScrollArea } from "@/shared/ui";

interface JsonCellViewerProps {
  value: Record<string, unknown>;
}

function highlightJson(json: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  // Match strings, numbers, booleans, null, property keys
  const regex =
    /("(?:\\.|[^"\\])*")\s*:|("(?:\\.|[^"\\])*")|(true|false)|(null)|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g;
  let lastIndex = 0;
  let key = 0;

  for (let match = regex.exec(json); match !== null; match = regex.exec(json)) {
    // Text before this match (punctuation, whitespace)
    if (match.index > lastIndex) {
      nodes.push(json.slice(lastIndex, match.index));
    }

    if (match[1] !== undefined) {
      // Property key
      nodes.push(
        <span key={key++} style={{ color: "var(--mantine-color-blue-4)" }}>
          {match[1]}
        </span>
      );
      nodes.push(": ");
    } else if (match[2] !== undefined) {
      // String value
      nodes.push(
        <span key={key++} style={{ color: "var(--mantine-color-green-5)" }}>
          {match[2]}
        </span>
      );
    } else if (match[3] !== undefined) {
      // Boolean
      nodes.push(
        <span key={key++} style={{ color: "var(--mantine-color-orange-5)" }}>
          {match[3]}
        </span>
      );
    } else if (match[4] !== undefined) {
      // Null
      nodes.push(
        <span key={key++} style={{ color: "var(--mantine-color-dimmed)" }}>
          {match[4]}
        </span>
      );
    } else if (match[5] !== undefined) {
      // Number
      nodes.push(
        <span key={key++} style={{ color: "var(--mantine-color-violet-5)" }}>
          {match[5]}
        </span>
      );
    }

    lastIndex = match.index + match[0].length;
  }

  // Remaining text
  if (lastIndex < json.length) {
    nodes.push(json.slice(lastIndex));
  }

  return nodes;
}

export function JsonCellViewer({ value }: JsonCellViewerProps) {
  const json = JSON.stringify(value, null, 2);

  return (
    <ScrollArea.Autosize mah={200} type="auto">
      <pre
        style={{
          margin: 0,
          fontSize: 11,
          fontFamily: "var(--mantine-font-family-monospace)",
          whiteSpace: "pre-wrap",
          wordBreak: "break-all",
        }}
      >
        {highlightJson(json)}
      </pre>
    </ScrollArea.Autosize>
  );
}
