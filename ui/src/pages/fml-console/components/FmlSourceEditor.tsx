/**
 * Monaco editor for FML: highlighting, the server's problems as markers, and the range of the
 * trace event being replayed. Ctrl/Cmd+Enter runs the map.
 */

import Editor, { type OnChange, type OnMount } from "@monaco-editor/react";
import { useColorScheme } from "@octofhir/ui-kit";
import type * as Monaco from "monaco-editor";
import { useCallback, useEffect, useRef } from "react";
import { bindEditorKey } from "@/shared/monaco/keybinding";
import {
  ensureOctofhirThemes,
  OCTOFHIR_THEME_DARK,
  OCTOFHIR_THEME_LIGHT,
} from "@/shared/monaco/lspClient";
import { ensureFmlLanguageRegistered, FML_LANGUAGE_ID } from "../fmlLanguage";
import type { Problem } from "../types";
import classes from "../FmlConsolePage.module.css";

ensureOctofhirThemes();
ensureFmlLanguageRegistered();

const MARKER_OWNER = "fml-check";

export interface EditorRange {
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
}

export interface FmlSourceEditorProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit?: () => void;
  problems: Problem[];
  /** The text of the trace event being replayed. */
  highlight?: EditorRange;
}

export function FmlSourceEditor({
  value,
  onChange,
  onSubmit,
  problems,
  highlight,
}: FmlSourceEditorProps) {
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<typeof Monaco | null>(null);
  const decorations = useRef<Monaco.editor.IEditorDecorationsCollection | null>(null);
  const onSubmitRef = useRef(onSubmit);
  onSubmitRef.current = onSubmit;
  const { colorScheme } = useColorScheme();

  const handleMount: OnMount = useCallback((editor, monaco) => {
    editorRef.current = editor;
    monacoRef.current = monaco;
    decorations.current = editor.createDecorationsCollection();
    bindEditorKey(editor, monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, "Run map", () => {
      onSubmitRef.current?.();
    });
  }, []);

  useEffect(() => {
    const editor = editorRef.current;
    const monaco = monacoRef.current;
    const model = editor?.getModel();
    if (!monaco || !model) return;
    monaco.editor.setModelMarkers(
      model,
      MARKER_OWNER,
      problems.map((problem) => ({
        severity:
          problem.severity === "error" ? monaco.MarkerSeverity.Error : monaco.MarkerSeverity.Warning,
        message: problem.message,
        startLineNumber: problem.startLine,
        startColumn: problem.startColumn,
        endLineNumber: problem.endLine,
        // An empty range would draw nothing: widen it to one character.
        endColumn:
          problem.endLine === problem.startLine && problem.endColumn <= problem.startColumn
            ? problem.startColumn + 1
            : problem.endColumn,
      }))
    );
  }, [problems]);

  useEffect(() => {
    const editor = editorRef.current;
    const monaco = monacoRef.current;
    if (!editor || !monaco || !decorations.current) return;
    if (!highlight) {
      decorations.current.clear();
      return;
    }
    const range = new monaco.Range(
      highlight.startLine,
      highlight.startColumn,
      highlight.endLine,
      highlight.endColumn
    );
    decorations.current.set([
      { range, options: { className: classes.traceHighlight, isWholeLine: false } },
    ]);
    editor.revealRangeInCenterIfOutsideViewport(range);
  }, [highlight]);

  const handleChange: OnChange = useCallback((next) => onChange(next ?? ""), [onChange]);

  return (
    <Editor
      height="100%"
      language={FML_LANGUAGE_ID}
      theme={colorScheme === "dark" ? OCTOFHIR_THEME_DARK : OCTOFHIR_THEME_LIGHT}
      value={value}
      onChange={handleChange}
      onMount={handleMount}
      options={{
        automaticLayout: true,
        minimap: { enabled: false },
        fontSize: 13,
        fontFamily: "var(--font-mono, 'JetBrains Mono', monospace)",
        lineNumbers: "on",
        scrollBeyondLastLine: false,
        padding: { top: 8, bottom: 8 },
        renderLineHighlight: "line",
        tabSize: 2,
        bracketPairColorization: { enabled: true },
      }}
    />
  );
}
