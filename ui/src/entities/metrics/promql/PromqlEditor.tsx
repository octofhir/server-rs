import Editor, { type OnMount } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import { useEffect, useRef } from "react";
import { bindEditorKey } from "@/shared/monaco/keybinding";
import { useOctoMonacoTheme } from "@/shared/monaco/theme";
import { metricsApi } from "../api";
import { clearPromqlVariables, ensurePromqlLanguageRegistered, PROMQL_LANGUAGE_ID, setPromqlVariables } from "./language";

const LINE_HEIGHT = 19;
const MAX_LINES = 12;
const MARKER_OWNER = "promql";
const CHECK_DELAY_MS = 400;

export interface PromqlEditorProps {
  value: string;
  onChange: (value: string) => void;
  /** ⌘/Ctrl+Enter. */
  onRun?: () => void;
  /** Dashboard variable names offered in completions. */
  variables?: string[];
  placeholder?: string;
  "aria-label"?: string;
}

/**
 * PromQL query input: grows with its content up to a dozen lines, completes metrics, labels,
 * values and functions, and underlines syntax errors as reported by the server's parser.
 */
export function PromqlEditor({ value, onChange, onRun, variables = [], placeholder, ...rest }: PromqlEditorProps) {
  const theme = useOctoMonacoTheme();
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<typeof Monaco | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const runRef = useRef(onRun);
  runRef.current = onRun;

  const resize = () => {
    const editor = editorRef.current;
    const container = containerRef.current;
    if (!editor || !container) return;
    const lines = Math.min(MAX_LINES, Math.max(1, editor.getModel()?.getLineCount() ?? 1));
    const height = lines * LINE_HEIGHT + 12;
    container.style.height = `${height}px`;
    editor.layout();
  };

  const handleMount: OnMount = (editor, monaco) => {
    editorRef.current = editor;
    monacoRef.current = monaco;
    const model = editor.getModel();
    if (model) setPromqlVariables(model, variables);
    bindEditorKey(editor, monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, "Run query", () => runRef.current?.());
    editor.onDidContentSizeChange(resize);
    resize();
  };

  useEffect(() => {
    const model = editorRef.current?.getModel();
    if (model) setPromqlVariables(model, variables);
  }, [variables]);

  useEffect(
    () => () => {
      const model = editorRef.current?.getModel();
      if (model) clearPromqlVariables(model);
    },
    []
  );

  // Diagnostics: ask the server's parser once typing pauses.
  useEffect(() => {
    const monaco = monacoRef.current;
    const model = editorRef.current?.getModel();
    if (!monaco || !model) return;
    if (value.trim() === "") {
      monaco.editor.setModelMarkers(model, MARKER_OWNER, []);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(async () => {
      try {
        const result = await metricsApi.parse(value);
        if (cancelled || model.isDisposed()) return;
        const error = result.error;
        if (!error) {
          monaco.editor.setModelMarkers(model, MARKER_OWNER, []);
          return;
        }
        const line = Math.min(error.line ?? 1, model.getLineCount());
        const column = error.column ?? 1;
        const word = model.getWordAtPosition({ lineNumber: line, column });
        monaco.editor.setModelMarkers(model, MARKER_OWNER, [
          {
            severity: monaco.MarkerSeverity.Error,
            message: error.message,
            startLineNumber: error.line ? line : 1,
            startColumn: error.column ? (word?.startColumn ?? column) : 1,
            endLineNumber: error.line ? line : model.getLineCount(),
            endColumn: error.column
              ? (word?.endColumn ?? column + 1)
              : model.getLineMaxColumn(model.getLineCount()),
          },
        ]);
      } catch {
        // The parse check is advisory; a failed request leaves the last markers.
      }
    }, CHECK_DELAY_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [value]);

  return (
    <div ref={containerRef} style={{ height: LINE_HEIGHT + 12, width: "100%" }} aria-label={rest["aria-label"]}>
      <Editor
        height="100%"
        language={PROMQL_LANGUAGE_ID}
        theme={theme}
        value={value}
        beforeMount={() => ensurePromqlLanguageRegistered()}
        onMount={handleMount}
        onChange={(next) => onChange(next ?? "")}
        options={{
          automaticLayout: true,
          minimap: { enabled: false },
          lineNumbers: "off",
          glyphMargin: false,
          folding: false,
          lineDecorationsWidth: 8,
          renderLineHighlight: "none",
          scrollBeyondLastLine: false,
          overviewRulerLanes: 0,
          scrollbar: { vertical: "auto", horizontal: "hidden", alwaysConsumeMouseWheel: false },
          fontSize: 13,
          lineHeight: LINE_HEIGHT,
          fontFamily: "var(--octo-typography-mono, 'JetBrains Mono', monospace)",
          wordWrap: "on",
          padding: { top: 6, bottom: 6 },
          fixedOverflowWidgets: true,
          suggestOnTriggerCharacters: true,
          quickSuggestions: { other: true, comments: false, strings: true },
          quickSuggestionsDelay: 80,
          hover: { enabled: true, delay: 300 },
          placeholder,
          tabSize: 2,
        }}
      />
    </div>
  );
}
