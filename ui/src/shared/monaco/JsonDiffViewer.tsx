import { DiffEditor } from "@monaco-editor/react";
import { useColorScheme } from "@octofhir/ui-kit";
import { ensureOctofhirThemes, OCTOFHIR_THEME_DARK, OCTOFHIR_THEME_LIGHT } from "./theme";

ensureOctofhirThemes();

export interface JsonDiffViewerProps {
  original: string;
  modified: string;
  height?: string | number;
  /** Side by side instead of one column with removed lines above added ones. */
  sideBySide?: boolean;
}

/** Read-only JSON diff that folds the unchanged parts away. */
export function JsonDiffViewer({ original, modified, height = "100%", sideBySide = false }: JsonDiffViewerProps) {
  const { colorScheme } = useColorScheme();
  return (
    <div style={{ height, width: "100%" }}>
      <DiffEditor
        height="100%"
        language="json"
        theme={colorScheme === "dark" ? OCTOFHIR_THEME_DARK : OCTOFHIR_THEME_LIGHT}
        original={original}
        modified={modified}
        options={{
          readOnly: true,
          originalEditable: false,
          renderSideBySide: sideBySide,
          useInlineViewWhenSpaceIsLimited: true,
          hideUnchangedRegions: { enabled: true, contextLineCount: 2, minimumLineCount: 4 },
          renderOverviewRuler: false,
          automaticLayout: true,
          minimap: { enabled: false },
          scrollBeyondLastLine: false,
          fontSize: 13,
          fontFamily: "var(--font-mono, 'JetBrains Mono', 'Fira Code', monospace)",
          lineNumbers: "off",
          glyphMargin: false,
          folding: false,
          wordWrap: "on",
          padding: { top: 8, bottom: 8 },
        }}
      />
    </div>
  );
}
