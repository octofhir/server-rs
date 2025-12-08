import { onCleanup, onMount, createEffect, on } from "solid-js";
import * as monaco from "monaco-editor";

// Configure Monaco workers via data URI to avoid worker loading issues
self.MonacoEnvironment = {
    getWorker(_workerId: string, _label: string) {
        // Return a minimal worker that does nothing but prevents errors
        const blob = new Blob(
            ['self.onmessage = function() {}'],
            { type: 'application/javascript' }
        );
        return new Worker(URL.createObjectURL(blob));
    },
};

export interface SqlEditorProps {
    /** Initial value of the editor */
    value?: string;
    /** Callback when content changes */
    onChange?: (value: string) => void;
    /** Callback for Ctrl+Enter key combination */
    onExecute?: () => void;
    /** Editor height (default: 100%) */
    height?: string;
    /** Whether the editor is read-only */
    readOnly?: boolean;
    /** Custom CSS class for the container */
    class?: string;
}

export function SqlEditor(props: SqlEditorProps) {
    let containerRef: HTMLDivElement | undefined;
    let editor: monaco.editor.IStandaloneCodeEditor | undefined;

    onMount(() => {
        if (!containerRef) return;

        // Create the Monaco editor
        editor = monaco.editor.create(containerRef, {
            value: props.value ?? "",
            language: "pgsql",
            theme: "vs-dark",
            automaticLayout: true,
            minimap: { enabled: false },
            lineNumbers: "on",
            renderLineHighlight: "line",
            scrollBeyondLastLine: false,
            fontSize: 14,
            fontFamily: "var(--font-mono, 'JetBrains Mono', 'Fira Code', monospace)",
            tabSize: 2,
            wordWrap: "on",
            readOnly: props.readOnly ?? false,
            padding: { top: 8, bottom: 8 },
            // SQL-specific settings
            suggestOnTriggerCharacters: true,
            quickSuggestions: true,
            acceptSuggestionOnEnter: "on",
        });

        // Handle content changes
        editor.onDidChangeModelContent(() => {
            if (editor && props.onChange) {
                props.onChange(editor.getValue());
            }
        });

        // Add Ctrl+Enter keybinding for execute
        editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => {
            props.onExecute?.();
        });

        // Focus the editor
        editor.focus();
    });

    // Update value from props when it changes externally
    createEffect(
        on(
            () => props.value,
            (newValue) => {
                if (editor && newValue !== undefined && newValue !== editor.getValue()) {
                    editor.setValue(newValue);
                }
            },
            { defer: true }
        )
    );

    // Update readOnly from props
    createEffect(
        on(
            () => props.readOnly,
            (readOnly) => {
                if (editor) {
                    editor.updateOptions({ readOnly: readOnly ?? false });
                }
            },
            { defer: true }
        )
    );

    onCleanup(() => {
        editor?.dispose();
    });

    return (
        <div
            ref={containerRef}
            class={props.class}
            style={{
                width: "100%",
                height: props.height ?? "100%",
                overflow: "hidden",
            }}
        />
    );
}
