/**
 * Monaco editor for CQL source — a single expression or a full library
 * (`library` / `using` / `context` / `parameter` / multiple `define`).
 *
 * CQL has no LSP server wired up, so this registers a lightweight Monarch
 * grammar for syntax highlighting and exposes an imperative `insertSnippet`
 * handle for the function palette. Ctrl/Cmd+Enter triggers submit.
 */

import Editor, { type OnChange, type OnMount } from "@monaco-editor/react";
import { useColorScheme } from "@octofhir/ui-kit";
import type * as Monaco from "monaco-editor";
import { forwardRef, useCallback, useImperativeHandle, useRef } from "react";
import {
  ensureOctofhirThemes,
  OCTOFHIR_THEME_DARK,
  OCTOFHIR_THEME_LIGHT,
} from "@/shared/monaco/lspClient";
import { CQL_LANGUAGE_ID, ensureCqlLanguageRegistered } from "../cqlLanguage";

ensureOctofhirThemes();
ensureCqlLanguageRegistered();

export interface CqlSourceEditorHandle {
  insertSnippet: (snippet: string) => void;
  focus: () => void;
}

export interface CqlSourceEditorProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit?: () => void;
  height?: string | number;
}

export const CqlSourceEditor = forwardRef<CqlSourceEditorHandle, CqlSourceEditorProps>(
  function CqlSourceEditor({ value, onChange, onSubmit, height = "100%" }, ref) {
    const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
    const onSubmitRef = useRef(onSubmit);
    onSubmitRef.current = onSubmit;
    const { colorScheme } = useColorScheme();

    useImperativeHandle(
      ref,
      () => ({
        insertSnippet: (snippet: string) => {
          const editor = editorRef.current;
          if (!editor) return;
          editor.focus();
          const controller = editor.getContribution("snippetController2") as {
            insert?: (template: string) => void;
          } | null;
          if (controller?.insert) {
            controller.insert(snippet);
          } else {
            editor.trigger("keyboard", "type", { text: snippet });
          }
        },
        focus: () => editorRef.current?.focus(),
      }),
      []
    );

    const handleMount: OnMount = useCallback((editor, monaco) => {
      editorRef.current = editor;
      ensureOctofhirThemes();
      ensureCqlLanguageRegistered();
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => {
        onSubmitRef.current?.();
      });
    }, []);

    const handleChange: OnChange = useCallback((newValue) => onChange(newValue ?? ""), [onChange]);

    const editorTheme = colorScheme === "dark" ? OCTOFHIR_THEME_DARK : OCTOFHIR_THEME_LIGHT;

    return (
      <Editor
        height={height}
        language={CQL_LANGUAGE_ID}
        theme={editorTheme}
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
          wordWrap: "on",
          tabCompletion: "on",
          bracketPairColorization: { enabled: true },
        }}
      />
    );
  }
);
