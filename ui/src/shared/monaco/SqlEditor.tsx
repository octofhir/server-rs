import { createEffect, on, onCleanup, onMount } from "solid-js";
// Use the full Monaco editor bundle instead of minimal API
// The minimal API (editor.api) doesn't include suggest widget and other contributions
import * as monaco from "monaco-editor";

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
	/** Whether to enable PostgreSQL LSP features */
	enableLsp?: boolean;
	/** Optional override for the LSP websocket path */
	lspPath?: string;
}

export function SqlEditor(props: SqlEditorProps) {
	let containerRef: HTMLDivElement | undefined;
	let editor: monaco.editor.IStandaloneCodeEditor | undefined;
	let disposeLsp: (() => void) | undefined;
	let disposeModelBinding: (() => void) | undefined;
	let model: monaco.editor.ITextModel | undefined;

	onMount(async () => {
		if (!containerRef) return;

		console.log("[SqlEditor] onMount - initializing editor");
		await ensureMonacoServices();
		ensurePgLanguageRegistered();

		const modelUri = monaco.Uri.parse(
			`inmemory://pg-console/${
				globalThis.crypto?.randomUUID?.() ?? Date.now()
			}.sql`,
		);
		console.log("[SqlEditor] Creating model with language:", PG_LANGUAGE_ID, "uri:", modelUri.toString());
		model = monaco.editor.createModel(
			props.value ?? "",
			PG_LANGUAGE_ID,
			modelUri,
		);
		console.log("[SqlEditor] Model created, languageId:", model.getLanguageId());

		editor = monaco.editor.create(containerRef, {
			model,
			language: PG_LANGUAGE_ID,
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
			suggestOnTriggerCharacters: true,
			quickSuggestions: {
				other: true,
				comments: false,
				strings: true,
			},
			acceptSuggestionOnEnter: "on",
			suggest: {
				showKeywords: true,
				showSnippets: true,
				showClasses: true,
				showFunctions: true,
				showVariables: true,
			},
		});
		console.log("[SqlEditor] Editor created with quickSuggestions enabled");

		editor.onDidChangeModelContent(() => {
			if (editor && props.onChange) {
				props.onChange(editor.getValue());
			}
		});

		editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => {
			props.onExecute?.();
		});

		if (props.enableLsp ?? true) {
			console.log("[SqlEditor] Starting LSP connection...");
			try {
				disposeLsp = await startPgLsp(() => buildPgLspUrl(props.lspPath));
				console.log("[SqlEditor] LSP started successfully");
				if (model) {
					console.log("[SqlEditor] Binding model to LSP...");
					disposeModelBinding = bindModelToLanguageServer(model);
					console.log("[SqlEditor] Model bound to LSP");
				}
			} catch (error) {
				console.warn("[SqlEditor] PostgreSQL LSP unavailable:", error);
			}
		}

		editor.focus();

		// Debug: Log all registered completion providers for this language
		console.log("[SqlEditor] Registered languages:", monaco.languages.getLanguages().map(l => l.id));
		console.log("[SqlEditor] Editor model language:", editor.getModel()?.getLanguageId());

		// Test: trigger suggestions programmatically after 2 seconds
		setTimeout(() => {
			console.log("[SqlEditor] Triggering suggestions programmatically...");
			editor?.trigger('keyboard', 'editor.action.triggerSuggest', {});
		}, 2000);
	});

	createEffect(
		on(
			() => props.value,
			(newValue) => {
				if (model && newValue !== undefined && newValue !== model.getValue()) {
					model.setValue(newValue);
				}
			},
			{ defer: true },
		),
	);

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
		disposeLsp?.();
		disposeModelBinding?.();
		stopPgLsp();
		editor?.dispose();
		model?.dispose();
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
