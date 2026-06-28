import { ScrollArea } from "@octofhir/ui-kit";
import { highlightJson } from "./jsonHighlight";

interface JsonCellViewerProps {
  value: Record<string, unknown>;
}

export function JsonCellViewer({ value }: JsonCellViewerProps) {
  const json = JSON.stringify(value, null, 2);

  return (
    <ScrollArea.Autosize mah={200} type="auto">
      <pre
        style={{
          margin: 0,
          fontSize: 11,
          fontFamily: "var(--octo-typography-mono)",
          whiteSpace: "pre-wrap",
          wordBreak: "break-all",
        }}
      >
        {highlightJson(json)}
      </pre>
    </ScrollArea.Autosize>
  );
}
