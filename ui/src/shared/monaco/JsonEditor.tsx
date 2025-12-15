import { useRef, useEffect, useCallback } from "react";
import Editor, { type OnMount, type OnChange } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";

export interface JsonEditorProps {
	/** Initial value of the editor */
	value?: string;
	/** Callback when content changes */
	onChange?: (value: string) => void;
	/** Callback for Ctrl+Enter key combination */
	onExecute?: () => void;
	/** Editor height (default: 100%) */
	height?: string | number;
	/** Whether the editor is read-only */
	readOnly?: boolean;
	/** Custom CSS class for the container */
	className?: string;
	/** Callback when JSON validation error occurs */
	onValidationError?: (error?: string) => void;
}

/**
 * React wrapper for Monaco JSON Editor.
 * Provides syntax highlighting, validation, and formatting for JSON content.
 */
export function JsonEditor({
	value = "",
	onChange,
	onExecute,
	height = "100%",
	readOnly = false,
	className,
	onValidationError,
}: JsonEditorProps) {
	const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
	const monacoRef = useRef<typeof Monaco | null>(null);

	// Setup Monaco when editor mounts
	const handleEditorDidMount: OnMount = useCallback(
		(editor, monaco) => {
			editorRef.current = editor;
			monacoRef.current = monaco;

			// Add Ctrl+Enter command for execute
			if (onExecute) {
				editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => {
					onExecute();
				});
			}

			// Focus the editor
			editor.focus();

			// Setup validation listener if callback provided
			if (onValidationError) {
				const model = editor.getModel();
				if (model) {
					// Listen for marker changes (validation errors)
					monaco.editor.onDidChangeMarkers(([resource]) => {
						if (model.uri.toString() === resource.toString()) {
							const markers = monaco.editor.getModelMarkers({ resource });
							if (markers.length > 0) {
								// Get first error message
								const firstError = markers[0];
								onValidationError(firstError.message);
							} else {
								onValidationError(undefined);
							}
						}
					});
				}
			}
		},
		[onExecute, onValidationError],
	);

	// Handle value changes from React
	const handleChange: OnChange = useCallback(
		(newValue) => {
			onChange?.(newValue ?? "");
		},
		[onChange],
	);

	// Cleanup on unmount
	useEffect(() => {
		return () => {
			editorRef.current?.dispose();
		};
	}, []);

	return (
		<div className={className} style={{ height, width: "100%" }}>
			<Editor
				height="100%"
				language="json"
				theme="vs-dark"
				value={value}
				onChange={handleChange}
				onMount={handleEditorDidMount}
				options={{
					automaticLayout: true,
					minimap: { enabled: false },
					lineNumbers: "on",
					renderLineHighlight: "line",
					scrollBeyondLastLine: false,
					fontSize: 13,
					fontFamily: "var(--font-mono, 'JetBrains Mono', 'Fira Code', monospace)",
					tabSize: 2,
					insertSpaces: true,
					wordWrap: "on",
					readOnly,
					padding: { top: 8, bottom: 8 },
					formatOnPaste: true,
					formatOnType: true,
					suggestOnTriggerCharacters: true,
					quickSuggestions: {
						other: true,
						comments: false,
						strings: true,
					},
					acceptSuggestionOnEnter: "on",
				}}
			/>
		</div>
	);
}
