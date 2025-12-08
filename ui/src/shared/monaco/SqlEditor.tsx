import { createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import * as monaco from "monaco-editor";
import { initializeLspClient, registerPgsqlLanguage } from "./lsp-client";

// Configure Monaco workers via data URI to avoid worker loading issues
self.MonacoEnvironment = {
	getWorker(_workerId: string, _label: string) {
		// Return a minimal worker that does nothing but prevents errors
		const blob = new Blob(["self.onmessage = function() {}"], {
			type: "application/javascript",
		});
		return new Worker(URL.createObjectURL(blob));
	},
};

// Track LSP initialization globally
let lspInitialized = false;

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
	/** Whether to enable LSP features (default: true) */
	enableLsp?: boolean;
	/** Document URI for LSP (default: auto-generated) */
	documentUri?: string;
}

export function SqlEditor(props: SqlEditorProps) {
	let containerRef: HTMLDivElement | undefined;
	let editor: monaco.editor.IStandaloneCodeEditor | undefined;
	let model: monaco.editor.ITextModel | undefined;
	const [lspStatus, setLspStatus] = createSignal<
		"disconnected" | "connecting" | "connected"
	>("disconnected");
	console.log(lspStatus());
	// Generate a unique document URI for this editor instance
	const documentUri = () =>
		props.documentUri ?? `file:///query-${Date.now()}.sql`;

	onMount(async () => {
		if (!containerRef) return;

		// Register pgsql language
		registerPgsqlLanguage();

		// Create a model for the editor with a proper URI
		const uri = monaco.Uri.parse(documentUri());
		model = monaco.editor.createModel(props.value ?? "", "pgsql", uri);

		// Create the Monaco editor with the model
		editor = monaco.editor.create(containerRef, {
			model,
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

		// Initialize LSP if enabled
		if (props.enableLsp !== false && !lspInitialized) {
			setLspStatus("connecting");
			try {
				await initializeLspClient();
				lspInitialized = true;
				setLspStatus("connected");
			} catch (error) {
				console.error("[SqlEditor] LSP initialization failed:", error);
				setLspStatus("disconnected");
			}
		} else if (lspInitialized) {
			setLspStatus("connected");
		}

		// Focus the editor
		editor.focus();
	});

	// Update value from props when it changes externally
	createEffect(
		on(
			() => props.value,
			(newValue) => {
				if (
					editor &&
					newValue !== undefined &&
					newValue !== editor.getValue()
				) {
					editor.setValue(newValue);
				}
			},
			{ defer: true },
		),
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
			{ defer: true },
		),
	);

	onCleanup(() => {
		model?.dispose();
		editor?.dispose();
	});

	return (
		<div
			style={{
				position: "relative",
				width: "100%",
				height: props.height ?? "100%",
			}}
		>
			<div
				ref={containerRef}
				class={props.class}
				style={{
					width: "100%",
					height: "100%",
					overflow: "hidden",
				}}
			/>
			{/* LSP status indicator */}
			<div
				style={{
					position: "absolute",
					bottom: "4px",
					right: "8px",
					"font-size": "10px",
					color:
						lspStatus() === "connected"
							? "#4ade80"
							: lspStatus() === "connecting"
								? "#fbbf24"
								: "#6b7280",
					"pointer-events": "none",
					"user-select": "none",
				}}
			>
				{lspStatus() === "connected" && "● LSP"}
				{lspStatus() === "connecting" && "○ LSP..."}
			</div>
		</div>
	);
}
