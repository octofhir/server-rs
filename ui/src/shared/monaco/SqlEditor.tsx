import { useRef, useEffect, useCallback } from "react";
import Editor, { type OnMount, type OnChange } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import { useMantineColorScheme } from "@octofhir/ui-kit";

// Monaco config is imported at app entry point (@/shared/monaco/config)

import {
	PG_LANGUAGE_ID,
	bindModelToLanguageServer,
	ensureMonacoServices,
	ensurePgLanguageRegistered,
	buildPgLspUrl,
	startPgLsp,
	stopPgLsp,
} from "./lspClient";

export interface SqlEditorProps {
	/** Controlled value of the editor */
	value?: string;
	/** Uncontrolled initial value */
	defaultValue?: string;
	/** Callback when content changes */
	onChange?: (value: string) => void;
	/** Callback for Ctrl+Enter key combination */
	onExecute?: (value: string) => void;
	/** Editor height (default: 100%) */
	height?: string | number;
	/** Whether the editor is read-only */
	readOnly?: boolean;
	/** Custom CSS class for the container */
	className?: string;
	/** Whether to enable PostgreSQL LSP features */
	enableLsp?: boolean;
	/** Optional override for the LSP websocket path */
	lspPath?: string;
	/** Callback when editor instance is available */
	onEditorMount?: (
		editor: Monaco.editor.IStandaloneCodeEditor,
		model: Monaco.editor.ITextModel,
	) => void;
}

/**
 * React wrapper for Monaco SQL Editor with PostgreSQL LSP support.
 * Uses the same LSP client as the SolidJS version.
 */
export function SqlEditor({
	value,
	defaultValue = "",
	onChange,
	onExecute,
	height = "100%",
	readOnly = false,
	className,
	enableLsp = true,
	lspPath,
	onEditorMount,
}: SqlEditorProps) {
	const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
	const monacoRef = useRef<typeof Monaco | null>(null);
	const disposeLspRef = useRef<(() => void) | null>(null);
	const disposeModelBindingRef = useRef<(() => void) | null>(null);
	const { colorScheme } = useMantineColorScheme();

	// Setup Monaco and LSP when editor mounts
	const handleEditorDidMount: OnMount = useCallback(
		async (editor, monaco) => {
			editorRef.current = editor;
			monacoRef.current = monaco;

			// Initialize Monaco services and register PostgreSQL language
			await ensureMonacoServices();
			ensurePgLanguageRegistered();

			// Add Ctrl+Enter command for execute and forward current value
			editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => {
				const currentValue = editor.getValue?.() ?? "";
				onExecute?.(currentValue);
			});

			// Start LSP connection if enabled
			if (enableLsp) {
				try {
					disposeLspRef.current = await startPgLsp(() =>
						buildPgLspUrl(lspPath),
					);

					// Bind model to LSP for completions and hover
					const model = editor.getModel();
					if (model) {
						disposeModelBindingRef.current = bindModelToLanguageServer(model);
					}
				} catch (error) {
					console.warn("[SqlEditor.react] PostgreSQL LSP unavailable:", error);
				}
			}

			// Notify parent component that editor is ready
			const model = editor.getModel();
			if (model) {
				onEditorMount?.(editor, model);
			}

			// Focus the editor
			editor.focus();
		},
		[enableLsp, lspPath, onExecute, onEditorMount],
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
			disposeLspRef.current?.();
			disposeModelBindingRef.current?.();
			stopPgLsp();
		};
	}, []);

	const editorValueProps =
		value === undefined
			? { defaultValue }
			: { value };

	// Determine theme based on color scheme
	const editorTheme = colorScheme === "dark" ? "vs-dark" : "vs";

	return (
		<div className={className} style={{ height, width: "100%" }}>
			<Editor
				height="100%"
				language={PG_LANGUAGE_ID}
				theme={editorTheme}
				{...editorValueProps}
				onChange={handleChange}
				onMount={handleEditorDidMount}
				options={{
					automaticLayout: true,
					minimap: { enabled: false },
					lineNumbers: "on",
					renderLineHighlight: "line",
					scrollBeyondLastLine: false,
					fontSize: 14,
					fontFamily:
						"var(--font-mono, 'JetBrains Mono', 'Fira Code', monospace)",
					tabSize: 2,
					wordWrap: "on",
					readOnly,
					padding: { top: 8, bottom: 8 },
					// Hover configuration - prevent overlay issues
					hover: {
						enabled: true,
						delay: 300,
						sticky: true,
						above: false, // Show hover below cursor to avoid overlay
					},
					// Fix overflow widgets to prevent clipping
					fixedOverflowWidgets: true,
					// Suggestions
					suggestOnTriggerCharacters: true,
					quickSuggestions: {
						other: true,
						comments: false,
						strings: true,
					},
					quickSuggestionsDelay: 100,
					acceptSuggestionOnEnter: "on",
					suggest: {
						showKeywords: true,
						showSnippets: true,
						showClasses: true,
						showFunctions: true,
						showVariables: true,
						showFields: true,
						showProperties: true,
						filterGraceful: false,
						snippetsPreventQuickSuggestions: false,
						localityBonus: false,
						shareSuggestSelections: false,
					},
				}}
			/>
		</div>
	);
}
