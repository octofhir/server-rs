/**
 * FHIRPath Single-Line Monaco Editor with LSP Support
 *
 * A specialized Monaco editor for FHIRPath expressions featuring:
 * - Single-line mode (32px height, no line numbers, minimal chrome)
 * - LSP-powered completion and diagnostics
 * - Context-aware suggestions (respects resourceType and constants)
 */
import { useRef, useEffect, useCallback, useState } from "react";
import Editor, { type OnMount, type OnChange } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import { useMantineColorScheme } from "@mantine/core";

import { ensureMonacoServices } from "./lspClient";
import {
	FHIRPATH_LANGUAGE_ID,
	bindFhirPathModelToLsp,
	buildFhirPathLspUrl,
	ensureFhirPathLanguageRegistered,
	setFhirPathContext,
	startFhirPathLsp,
	stopFhirPathLsp,
	type ConstantInfo,
} from "./fhirpathLspClient";

// Re-export for convenience
export type { ConstantInfo };

export interface FhirPathEditorProps {
	/** Current value of the editor */
	value?: string;
	/** Default value (used only on mount) */
	defaultValue?: string;
	/** Called when the editor content changes */
	onChange?: (value: string) => void;
	/** Called when the editor loses focus */
	onBlur?: () => void;
	/** Called when Enter is pressed (for single-line mode) */
	onSubmit?: () => void;
	/** Resource type context for completions (e.g., "Patient", "HumanName") */
	resourceType?: string;
	/** External constants available in expressions */
	constants?: Record<string, ConstantInfo>;
	/** Path context for nested expressions (e.g., ["name"] for columns inside a forEach on name) */
	forEachContext?: string[];
	/** Placeholder text shown when editor is empty */
	placeholder?: string;
	/** Whether the editor is read-only */
	readOnly?: boolean;
	/** Whether to focus the editor on mount */
	autoFocus?: boolean;
	/** Enable LSP features (completion, diagnostics) */
	enableLsp?: boolean;
	/** Additional CSS class name */
	className?: string;
	/** Custom height (default: 32px for single-line) */
	height?: number | string;
}

// Track global LSP connection state
let lspStarted = false;
let lspStartPromise: Promise<() => void> | null = null;

async function ensureLspStarted(): Promise<void> {
	if (lspStarted) return;

	if (!lspStartPromise) {
		lspStartPromise = startFhirPathLsp(buildFhirPathLspUrl);
		try {
			await lspStartPromise;
			lspStarted = true;
		} catch (error) {
			lspStartPromise = null;
			console.warn("[FhirPathEditor] LSP unavailable:", error);
		}
	} else {
		await lspStartPromise;
	}
}

/**
 * Single-line Monaco editor for FHIRPath expressions with LSP support.
 *
 * Features:
 * - Single-line mode (Enter submits/blurs, no line numbers)
 * - LSP-powered completion and diagnostics
 * - Context-aware (respects resourceType and constants)
 * - Minimal chrome for inline use
 */
export function FhirPathEditor({
	value,
	defaultValue = "",
	onChange,
	onBlur,
	onSubmit,
	resourceType,
	constants,
	forEachContext,
	placeholder = "FHIRPath expression",
	readOnly = false,
	autoFocus = false,
	enableLsp = true,
	className,
	height = 32,
}: FhirPathEditorProps) {
	const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
	const monacoRef = useRef<typeof Monaco | null>(null);
	const disposeModelRef = useRef<(() => void) | null>(null);
	const [editorReady, setEditorReady] = useState(false);
	const { colorScheme } = useMantineColorScheme();

	// Update LSP context when resourceType, constants, or forEachContext change
	useEffect(() => {
		if (editorReady && enableLsp && lspStarted) {
			setFhirPathContext({ resourceType, constants, forEachContext });
		}
	}, [resourceType, constants, forEachContext, editorReady, enableLsp]);

	const handleMount: OnMount = useCallback(
		async (editor, monaco) => {
			editorRef.current = editor;
			monacoRef.current = monaco;

			await ensureMonacoServices();
			ensureFhirPathLanguageRegistered();

			// Disable multi-line (treat Enter as submit/blur)
			editor.addCommand(monaco.KeyCode.Enter, () => {
				// First try to accept any active suggestion
				const suggestController = editor.getContribution(
					"editor.contrib.suggestController",
				) as { acceptSelectedSuggestion?: () => void } | null;

				if (suggestController?.acceptSelectedSuggestion) {
					suggestController.acceptSelectedSuggestion();
				}

				// If submit callback provided, call it, otherwise blur
				setTimeout(() => {
					if (onSubmit) {
						onSubmit();
					} else {
						onBlur?.();
					}
				}, 50);
			});

			// Handle Escape to blur
			editor.addCommand(monaco.KeyCode.Escape, () => {
				editor.trigger("keyboard", "hideSuggestWidget", {});
				onBlur?.();
			});

			// Start LSP if enabled
			if (enableLsp) {
				try {
					await ensureLspStarted();

					const model = editor.getModel();
					if (model) {
						disposeModelRef.current = bindFhirPathModelToLsp(model);
					}

					// Set initial context
					setFhirPathContext({ resourceType, constants, forEachContext });
				} catch (error) {
					console.warn("[FhirPathEditor] LSP unavailable:", error);
				}
			}

			setEditorReady(true);

			if (autoFocus) {
				editor.focus();
			}
		},
		[enableLsp, resourceType, constants, forEachContext, autoFocus, onBlur, onSubmit],
	);

	const handleChange: OnChange = useCallback(
		(newValue) => {
			onChange?.(newValue ?? "");
		},
		[onChange],
	);

	// Handle blur event
	const handleBlur = useCallback(() => {
		onBlur?.();
	}, [onBlur]);

	// Cleanup on unmount
	useEffect(() => {
		return () => {
			disposeModelRef.current?.();
		};
	}, []);

	// Monitor editor blur
	useEffect(() => {
		const editor = editorRef.current;
		if (!editor) return;

		const disposable = editor.onDidBlurEditorWidget(() => {
			handleBlur();
		});

		return () => disposable.dispose();
	}, [handleBlur]);

	const editorTheme = colorScheme === "dark" ? "vs-dark" : "vs";

	return (
		<div
			className={className}
			style={{
				height: typeof height === "number" ? `${height}px` : height,
				width: "100%",
				position: "relative",
			}}
		>
			<Editor
				height="100%"
				language={FHIRPATH_LANGUAGE_ID}
				theme={editorTheme}
				value={value}
				defaultValue={defaultValue}
				onChange={handleChange}
				onMount={handleMount}
				options={{
					// Single-line mode settings
					automaticLayout: true,
					lineNumbers: "off",
					lineDecorationsWidth: 0,
					lineNumbersMinChars: 0,
					glyphMargin: false,
					folding: false,
					minimap: { enabled: false },
					scrollbar: {
						vertical: "hidden",
						horizontal: "auto",
						useShadows: false,
						horizontalScrollbarSize: 4,
					},
					overviewRulerLanes: 0,
					overviewRulerBorder: false,
					hideCursorInOverviewRuler: true,
					wordWrap: "off",
					scrollBeyondLastLine: false,
					scrollBeyondLastColumn: 0,
					renderLineHighlight: "none",
					renderLineHighlightOnlyWhenFocus: true,

					// Editor behavior
					fontSize: 13,
					fontFamily: "var(--font-mono, 'JetBrains Mono', monospace)",
					readOnly,
					padding: { top: 6, bottom: 6 },
					cursorBlinking: "smooth",
					cursorStyle: "line",

					// Completions - fast autocomplete with immediate trigger after dot
					suggestOnTriggerCharacters: true,
					quickSuggestions: {
						other: true,
						comments: false,
						strings: false,
					},
					quickSuggestionsDelay: 0,
					acceptSuggestionOnEnter: "on",
					tabCompletion: "on",
					suggest: {
						showKeywords: true,
						showFunctions: true,
						showConstants: true,
						showProperties: true,
						filterGraceful: true,
						preview: true,
						previewMode: "subword",
					},
					inlineSuggest: { enabled: false },

					// Disable features not needed for single-line
					find: {
						addExtraSpaceOnTop: false,
						autoFindInSelection: "never",
					},
					links: false,
					contextmenu: false,
					fixedOverflowWidgets: true,

					// Placeholder via aria-label
					ariaLabel: placeholder,
				}}
			/>
		</div>
	);
}

// Cleanup function for when the app unmounts
export function disposeFhirPathLsp(): void {
	stopFhirPathLsp();
	lspStarted = false;
	lspStartPromise = null;
}
