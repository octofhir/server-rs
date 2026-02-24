import { useRef, useEffect, useCallback } from "react";
import Editor, { type OnMount, type OnChange } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import { useMantineColorScheme } from "@octofhir/ui-kit";
import {
	registerFhirQueryLanguage,
	LANGUAGE_ID,
	registerCompletionProvider,
	registerHoverProvider,
	updateDiagnosticsFromContent,
} from "../adapters/monaco";
import type { QueryInputMetadata } from "../core/types";

export interface QueryEditorProps {
	value: string;
	onChange: (value: string) => void;
	onExecute?: () => void;
	metadata: QueryInputMetadata;
	basePath?: string;
	disabled?: boolean;
	/** When true, removes border/radius — for embedding inside a parent container */
	borderless?: boolean;
}

export function QueryEditor({
	value,
	onChange,
	onExecute,
	metadata,
	basePath = "/fhir",
	disabled = false,
	borderless = false,
}: QueryEditorProps) {
	const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
	const monacoRef = useRef<typeof Monaco | null>(null);
	const disposablesRef = useRef<Monaco.IDisposable[]>([]);
	const metadataRef = useRef(metadata);
	metadataRef.current = metadata;
	const onExecuteRef = useRef(onExecute);
	onExecuteRef.current = onExecute;

	const { colorScheme } = useMantineColorScheme();
	const editorTheme = colorScheme === "dark" ? "vs-dark" : "vs";

	const handleMount: OnMount = useCallback(
		(editor, monaco) => {
			editorRef.current = editor;
			monacoRef.current = monaco;

			// Register language and providers
			registerFhirQueryLanguage(monaco);

			const getMetadata = () => metadataRef.current;

			disposablesRef.current.push(
				registerCompletionProvider(monaco, getMetadata, basePath),
				registerHoverProvider(monaco, getMetadata, basePath),
			);

			// Prevent Enter from inserting newline — single-line mode
			// Only block when suggest widget is NOT visible so Enter can still accept completions
			editor.addCommand(
				monaco.KeyCode.Enter,
				() => {
					// Do nothing — keep single line
				},
				"!suggestWidgetVisible",
			);

			// Ctrl+Enter to execute (use ref to avoid stale closure)
			editor.addCommand(
				monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter,
				() => onExecuteRef.current?.(),
			);

			// Initial diagnostics
			const model = editor.getModel();
			if (model) {
				updateDiagnosticsFromContent(monaco, model, metadataRef.current, basePath);
			}

			editor.focus();
		},
		[basePath],
	);

	const handleChange: OnChange = useCallback(
		(newValue) => {
			const val = (newValue ?? "").replace(/\n/g, ""); // Strip any newlines
			onChange(val);

			// Update diagnostics
			if (monacoRef.current && editorRef.current) {
				const model = editorRef.current.getModel();
				if (model) {
					updateDiagnosticsFromContent(
						monacoRef.current,
						model,
						metadataRef.current,
						basePath,
					);
				}
			}
		},
		[onChange, basePath],
	);

	// Update diagnostics when metadata changes
	useEffect(() => {
		if (monacoRef.current && editorRef.current) {
			const model = editorRef.current.getModel();
			if (model) {
				updateDiagnosticsFromContent(
					monacoRef.current,
					model,
					metadataRef.current,
					basePath,
				);
			}
		}
	}, [metadata, basePath]);

	// Cleanup
	useEffect(() => {
		return () => {
			for (const d of disposablesRef.current) d.dispose();
			disposablesRef.current = [];
		};
	}, []);

	return (
		<div
			style={{
				height: 36,
				width: "100%",
				...(borderless
					? {}
					: {
							borderRadius: "var(--mantine-radius-md)",
							border: "1px solid var(--octo-border-subtle)",
						}),
				position: "relative",
			}}
		>
			<Editor
				height="100%"
				language={LANGUAGE_ID}
				theme={editorTheme}
				value={value}
				onChange={handleChange}
				onMount={handleMount}
				options={{
					automaticLayout: true,
					minimap: { enabled: false },
					lineNumbers: "off",
					glyphMargin: false,
					folding: false,
					lineDecorationsWidth: 8,
					lineNumbersMinChars: 0,
					renderLineHighlight: "none",
					scrollBeyondLastLine: false,
					scrollbar: {
						horizontal: "hidden",
						vertical: "hidden",
					},
					overviewRulerLanes: 0,
					overviewRulerBorder: false,
					hideCursorInOverviewRuler: true,
					fixedOverflowWidgets: true,
					fontSize: 13,
					fontFamily:
						"var(--font-mono, 'JetBrains Mono', 'Fira Code', monospace)",
					wordWrap: "off",
					readOnly: disabled,
					padding: { top: 6, bottom: 6 },
					suggestOnTriggerCharacters: true,
					quickSuggestions: true,
					acceptSuggestionOnEnter: "on",
					tabCompletion: "on",
					contextmenu: false,
					find: { addExtraSpaceOnTop: false, autoFindInSelection: "never" },
				}}
			/>
		</div>
	);
}
