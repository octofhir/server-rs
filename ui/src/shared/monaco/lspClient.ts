// Use the full Monaco editor bundle - minimal API doesn't include suggest widget
import * as monaco from "monaco-editor";
import {
	conf as sqlConfiguration,
	language as sqlLanguage,
} from "monaco-editor/esm/vs/basic-languages/sql/sql";
import type { FormatterConfig } from "../settings/formatterTypes";

const DEFAULT_LSP_PATH = "/api/lsp/pg";
const PG_SPECIFIC_KEYWORDS = [
	"jsonb_each",
	"jsonb_array_elements",
	"->",
	"->>",
	"#>",
	"#>>",
	"@>",
	"<@",
	"?",
	"?|",
	"?&",
	"@?",
	"@@",
	"coalesce",
	"nullif",
	"ilike",
	"similar",
	"unnest",
	"lateral",
	"exists",
];

const textDecoder = new TextDecoder();

let monacoReady: Promise<void> | undefined;
let languageRegistered = false;
let activeConnection: PgLspConnection | undefined;
let currentFormatterConfig: FormatterConfig | undefined;

export const PG_LANGUAGE_ID = "pgsql";

type WorkerFactory = () => Worker;

const workerMap: Record<string, WorkerFactory> = {
	editor: () =>
		new Worker(
			new URL("monaco-editor/esm/vs/editor/editor.worker.js", import.meta.url),
			{ type: "module" },
		),
	typescript: () =>
		new Worker(
			new URL(
				"monaco-editor/esm/vs/language/typescript/ts.worker.js",
				import.meta.url,
			),
			{ type: "module" },
		),
	javascript: () =>
		new Worker(
			new URL(
				"monaco-editor/esm/vs/language/typescript/ts.worker.js",
				import.meta.url,
			),
			{ type: "module" },
		),
	// SQL/PostgreSQL uses the base editor worker (no specific SQL worker available)
};

export async function ensureMonacoServices(): Promise<void> {
	if (monacoReady) {
		return monacoReady;
	}

	monacoReady = Promise.resolve().then(() => {
		if (typeof window === "undefined") {
			return;
		}

		if (!window.MonacoEnvironment) {
			window.MonacoEnvironment = {
				getWorker(_workerId, label) {
					if (label && label in workerMap) {
						return workerMap[label]();
					}
					return workerMap.editor();
				},
			};
		}
	});

	return monacoReady;
}

export function ensurePgLanguageRegistered(): void {
	// Check if language is already registered in Monaco (not just our flag)
	const existingLanguages = monaco.languages.getLanguages();
	const alreadyRegistered = existingLanguages.some(
		(l) => l.id === PG_LANGUAGE_ID,
	);

	if (languageRegistered || alreadyRegistered) {
		languageRegistered = true;
		return;
	}

	const keywords = Array.from(
		new Set([...(sqlLanguage.keywords ?? []), ...PG_SPECIFIC_KEYWORDS]),
	);

	monaco.languages.register({
		id: PG_LANGUAGE_ID,
		extensions: [".sql", ".pgsql", ".psql"],
		aliases: ["PostgreSQL", "SQL"],
	});

	monaco.languages.setLanguageConfiguration(PG_LANGUAGE_ID, {
		...sqlConfiguration,
		comments: {
			lineComment: "--",
			blockComment: ["/*", "*/"],
		},
		autoClosingPairs: [
			{ open: "{", close: "}" },
			{ open: "[", close: "]" },
			{ open: "(", close: ")" },
			{ open: "'", close: "'", notIn: ["string", "comment"] },
			{ open: '"', close: '"', notIn: ["string"] },
		],
		surroundingPairs: [
			{ open: "{", close: "}" },
			{ open: "[", close: "]" },
			{ open: "(", close: ")" },
			{ open: "'", close: "'" },
			{ open: '"', close: '"' },
		],
		brackets: [
			["{", "}"],
			["[", "]"],
			["(", ")"],
		],
		onEnterRules: [
			{
				beforeText:
					/^\s*(?:SELECT|FROM|WHERE|JOIN|LEFT|RIGHT|INNER|GROUP|ORDER|HAVING)\b/i,
				action: { indentAction: monaco.languages.IndentAction.Indent },
			},
		],
	});

	const pgLanguage = {
		...sqlLanguage,
		keywords,
	};
	monaco.languages.setMonarchTokensProvider(PG_LANGUAGE_ID, pgLanguage);

	languageRegistered = true;
}

export function buildPgLspUrl(pathOverride?: string): string {
	const path = pathOverride || DEFAULT_LSP_PATH;
	const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
	return `${protocol}//${window.location.host}${path}`;
}

export async function startPgLsp(getUrl: () => string): Promise<() => void> {
	if (activeConnection) {
		return () => stopPgLsp();
	}

	const connection = new PgLspConnection(getUrl);
	activeConnection = connection;

	try {
		await connection.start();
	} catch (error) {
		connection.dispose();
		activeConnection = undefined;
		throw error;
	}

	return () => stopPgLsp();
}

export function stopPgLsp(): void {
	activeConnection?.dispose();
	activeConnection = undefined;
}

export function bindModelToLanguageServer(
	model: monaco.editor.ITextModel,
): () => void {
	if (!activeConnection) {
		return () => {};
	}
	return activeConnection.bindModel(model);
}

/**
 * Set the formatter configuration to use for LSP formatting requests.
 * This config is passed in the options of textDocument/formatting requests.
 *
 * @param config - The formatter configuration to use
 */
export function setLspFormatterConfig(config: FormatterConfig | undefined): void {
	currentFormatterConfig = config;
}

/**
 * Get the current formatter configuration.
 *
 * @returns The current formatter configuration or undefined
 */
export function getLspFormatterConfig(): FormatterConfig | undefined {
	return currentFormatterConfig;
}

type LspPosition = { line: number; character: number };
type LspRange = { start: LspPosition; end: LspPosition };

interface LspTextEdit {
	range: LspRange;
	newText: string;
}

interface LspCompletionItem {
	label: string;
	kind?: number;
	detail?: string | null;
	documentation?: string | { kind?: string; value: string };
	insertText?: string;
	insertTextFormat?: number;
	textEdit?: LspTextEdit;
	filterText?: string;
	sortText?: string;
}

interface LspCompletionList {
	isIncomplete?: boolean;
	items: LspCompletionItem[];
}

type LspCompletionResponse = LspCompletionItem[] | LspCompletionList;

interface LspHover {
	contents: unknown;
	range?: LspRange;
}

interface LspDiagnostic {
	range: LspRange;
	severity?: number;
	code?: string | number;
	source?: string;
	message: string;
}

interface LspPublishDiagnosticsParams {
	uri: string;
	diagnostics: LspDiagnostic[];
}

interface PendingRequest {
	resolve: (value: unknown) => void;
	reject: (error: Error) => void;
	timeoutId: number;
	method: string;
}

interface TrackedDocument {
	uri: string;
	model: monaco.editor.ITextModel;
	listener: monaco.IDisposable;
}

class PgLspConnection {
	private socket: WebSocket | undefined;
	private initialized = false;
	private disposed = false;
	private reconnectDelay = 1000;
	private reconnectHandle: number | undefined;
	private readyPromise: Promise<void> | null = null;
	private readyResolve: (() => void) | undefined;
	private readyReject: ((err: Error) => void) | undefined;
	private requestId = 0;
	private readonly pendingRequests = new Map<number, PendingRequest>();
	private readonly documents = new Map<string, TrackedDocument>();
	private readonly notificationQueue: Array<{
		method: string;
		params?: unknown;
	}> = [];
	private languageFeatureDisposables: monaco.IDisposable[] = [];
	private shouldReconnect = true;

	constructor(private readonly urlFactory: () => string) {}

	public async start(): Promise<void> {
		this.languageFeatureDisposables = this.registerLanguageFeatures();
		await this.connect();
	}

	public dispose(): void {
		this.disposed = true;
		this.shouldReconnect = false;
		for (const d of this.languageFeatureDisposables) {
			d.dispose();
		}
		this.languageFeatureDisposables = [];

		for (const doc of this.documents.values()) {
			doc.listener.dispose();
		}
		this.documents.clear();

		this.rejectAllPendingRequests(new Error("LSP connection disposed"));

		if (this.reconnectHandle !== undefined) {
			clearTimeout(this.reconnectHandle);
			this.reconnectHandle = undefined;
		}

		if (this.socket) {
			this.socket.close();
			this.socket = undefined;
		}
	}

	public bindModel(model: monaco.editor.ITextModel): () => void {
		const uri = model.uri.toString();

		const listener = model.onDidChangeContent(() => {
			this.sendDidChange(model);
		});

		const tracked: TrackedDocument = {
			uri,
			model,
			listener,
		};

		this.documents.set(uri, tracked);
		this.sendDidOpen(model);

		return () => {
			listener.dispose();
			this.documents.delete(uri);
			this.sendNotification("textDocument/didClose", {
				textDocument: { uri },
			});
		};
	}

	private async connect(): Promise<void> {
		if (this.disposed) return;

		const url = this.urlFactory();

		this.initialized = false;
		this.readyPromise = new Promise((resolve, reject) => {
			this.readyResolve = resolve;
			this.readyReject = reject;
		});

		const socket = new WebSocket(url);
		this.socket = socket;

		socket.onopen = () => {
			this.reconnectDelay = 1000;
			this.sendInitialize().catch((error) => {
				this.readyReject?.(
					error instanceof Error ? error : new Error(String(error)),
				);
			});
		};

		socket.onmessage = (event) => {
			this.handleMessage(event.data);
		};

		socket.onerror = (event) => {
			logDebug("[pg-lsp] socket error", event);
		};

		socket.onclose = (event) => {
			logDebug(
				`[pg-lsp] socket closed ${event.code} ${event.reason || ""}`.trim(),
			);
			this.initialized = false;
			this.socket = undefined;
			this.notificationQueue.length = 0;
			if (this.readyReject) {
				this.readyReject(
					new Error(`LSP connection closed before ready (${event.code})`),
				);
			}
			this.readyPromise = null;
			this.readyResolve = undefined;
			this.readyReject = undefined;
			this.rejectAllPendingRequests(
				new Error(`LSP socket closed (${event.code})`),
			);

			if (!this.disposed && this.shouldReconnect) {
				this.scheduleReconnect();
			}
		};

		return this.waitUntilReady();
	}

	private async sendInitialize(): Promise<void> {
		const params = {
			processId: null,
			rootUri: null,
			capabilities: {
				textDocument: {
					synchronization: {
						didSave: false,
						willSaveWaitUntil: false,
						willSave: false,
						dynamicRegistration: false,
					},
					completion: {
						completionItem: {
							snippetSupport: true,
						},
					},
					hover: {
						contentFormat: ["markdown", "plaintext"],
					},
					formatting: {
						dynamicRegistration: false,
					},
				},
			},
			clientInfo: {
				name: "octofhir-ui",
				version: "0.1.0",
			},
			workspaceFolders: null,
		};

		await this.sendRequest("initialize", params, true);
		this.markReady();
		this.rawSend({
			method: "initialized",
			params: {},
		});
		this.flushNotificationQueue();
		for (const doc of this.documents.values()) {
			this.sendDidOpen(doc.model);
		}
	}

	private markReady() {
		this.initialized = true;
		this.readyResolve?.();
		this.readyResolve = undefined;
		this.readyReject = undefined;
		this.readyPromise = Promise.resolve();
	}

	private scheduleReconnect(): void {
		if (this.reconnectHandle !== undefined || this.disposed) {
			return;
		}

		const delay = this.reconnectDelay;
		logDebug(`[pg-lsp] reconnecting in ${delay}ms`);

		this.reconnectHandle = window.setTimeout(() => {
			this.reconnectHandle = undefined;
			this.connect().catch((error) => {
				logDebug("[pg-lsp] reconnect failed", error);
				this.scheduleReconnect();
			});
		}, delay);

		this.reconnectDelay = Math.min(this.reconnectDelay * 2, 30000);
	}

	private waitUntilReady(): Promise<void> {
		if (this.initialized) {
			return Promise.resolve();
		}
		if (!this.readyPromise) {
			this.readyPromise = new Promise((resolve, reject) => {
				this.readyResolve = resolve;
				this.readyReject = reject;
			});
		}
		return this.readyPromise;
	}

	private rejectAllPendingRequests(error: Error): void {
		for (const pending of this.pendingRequests.values()) {
			clearTimeout(pending.timeoutId);
			pending.reject(error);
		}
		this.pendingRequests.clear();
	}

	private handleMessage(data: string | ArrayBuffer | Blob): void {
		if (data instanceof Blob) {
			data
				.text()
				.then((text) => this.handleMessage(text))
				.catch((error) =>
					logDebug("[pg-lsp] failed to read blob message", error),
				);
			return;
		}

		const text =
			typeof data === "string" ? data : textDecoder.decode(data as ArrayBuffer);

		logDebug("[pg-lsp] <= ", text);

		let message: any;
		try {
			message = JSON.parse(text);
		} catch (error) {
			logDebug("[pg-lsp] failed to parse message", error);
			return;
		}

		if (typeof message.id !== "undefined") {
			const pending = this.pendingRequests.get(Number(message.id));
			if (!pending) return;

			this.pendingRequests.delete(Number(message.id));
			clearTimeout(pending.timeoutId);

			if (message.error) {
				const msg =
					message.error.message ?? `LSP request ${pending.method} failed`;
				pending.reject(new Error(msg));
			} else {
				pending.resolve(message.result);
			}
			return;
		}

		if (message.method === "window/logMessage") {
			const params = message.params ?? {};
			logDebug(`[pg-lsp] server: ${params.message ?? "log message"}`);
			return;
		}

		if (message.method === "window/showMessage") {
			// Silently ignore window/showMessage notifications
			return;
		}

		if (message.method === "textDocument/publishDiagnostics") {
			this.handleDiagnostics(message.params);
			return;
		}
	}

	private handleDiagnostics(params: LspPublishDiagnosticsParams): void {
		if (!params || !params.uri) {
			console.warn("[pg-lsp] handleDiagnostics: no params or uri");
			return;
		}

		const uri = params.uri;
		const diagnostics = params.diagnostics || [];

		console.log(
			`[pg-lsp] Received ${diagnostics.length} diagnostics for ${uri}`,
			diagnostics,
		);

		// Find the model for this URI
		const doc = this.documents.get(uri);
		if (!doc || doc.model.isDisposed()) {
			console.warn(
				`[pg-lsp] Document not found or disposed for uri: ${uri}`,
				"Available documents:",
				Array.from(this.documents.keys()),
			);
			return;
		}

		// Convert LSP diagnostics to Monaco markers
		const markers = diagnostics.map((diag) => {
			// Convert LSP severity to Monaco severity
			// LSP: Error=1, Warning=2, Information=3, Hint=4
			// Monaco: Error=8, Warning=4, Info=2, Hint=1
			let severity = monaco.MarkerSeverity.Error;
			if (diag.severity === 2) {
				severity = monaco.MarkerSeverity.Warning;
			} else if (diag.severity === 3) {
				severity = monaco.MarkerSeverity.Info;
			} else if (diag.severity === 4) {
				severity = monaco.MarkerSeverity.Hint;
			}

			return {
				severity,
				startLineNumber: diag.range.start.line + 1,
				startColumn: diag.range.start.character + 1,
				endLineNumber: diag.range.end.line + 1,
				endColumn: diag.range.end.character + 1,
				message: diag.message,
				source: diag.source || "lsp",
				code: diag.code ? String(diag.code) : undefined,
			};
		});

		console.log(
			`[pg-lsp] Setting ${markers.length} markers on model ${uri}`,
			markers,
		);

		// Set markers on the model
		monaco.editor.setModelMarkers(doc.model, "pg-lsp", markers);

		console.log(
			`[pg-lsp] Markers set successfully. Total markers for model:`,
			monaco.editor.getModelMarkers({ resource: doc.model.uri }),
		);
	}

	private async sendRequest<T>(
		method: string,
		params?: unknown,
		allowDuringInit = false,
	): Promise<T> {
		if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
			throw new Error("LSP socket is not ready");
		}

		if (!this.initialized && !allowDuringInit) {
			await this.waitUntilReady();
		}

		const id = ++this.requestId;
		const payload = JSON.stringify({
			jsonrpc: "2.0",
			id,
			method,
			params,
		});

		logDebug("[pg-lsp] => ", payload);
		this.socket.send(payload);

		return new Promise<T>((resolve, reject) => {
			const timeoutId = window.setTimeout(() => {
				this.pendingRequests.delete(id);
				reject(new Error(`LSP request ${method} timed out after 15s`));
			}, 15000);

			this.pendingRequests.set(id, {
				resolve,
				reject,
				timeoutId,
				method,
			});
		});
	}

	private rawSend(message: { method: string; params?: unknown }): void {
		if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
			return;
		}

		const payload = JSON.stringify({
			jsonrpc: "2.0",
			...message,
		});
		logDebug("[pg-lsp] => ", payload);
		this.socket.send(payload);
	}

	private sendNotification(method: string, params?: unknown): void {
		if (
			!this.socket ||
			this.socket.readyState !== WebSocket.OPEN ||
			!this.initialized
		) {
			this.notificationQueue.push({ method, params });
			return;
		}
		this.rawSend({ method, params });
	}

	private flushNotificationQueue(): void {
		if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
			return;
		}
		while (this.notificationQueue.length > 0) {
			const item = this.notificationQueue.shift();
			if (!item) break;
			this.rawSend(item);
		}
	}

	private sendDidOpen(model: monaco.editor.ITextModel): void {
		if (model.isDisposed()) return;
		this.sendNotification("textDocument/didOpen", {
			textDocument: {
				uri: model.uri.toString(),
				languageId: PG_LANGUAGE_ID,
				version: model.getVersionId(),
				text: model.getValue(),
			},
		});
	}

	private sendDidChange(model: monaco.editor.ITextModel): void {
		if (model.isDisposed()) return;
		this.sendNotification("textDocument/didChange", {
			textDocument: {
				uri: model.uri.toString(),
				version: model.getVersionId(),
			},
			contentChanges: [{ text: model.getValue() }],
		});
	}

	private registerLanguageFeatures(): monaco.IDisposable[] {
		const disposables: monaco.IDisposable[] = [];

		const completionProvider = monaco.languages.registerCompletionItemProvider(
			PG_LANGUAGE_ID,
			{
				triggerCharacters: [
					".",
					">",
					":",
					" ",
					"(",
					"'",
					'"',
					"{",
					",",
					"$",
					"[",
				],
				provideCompletionItems: async (model, position) => {
					try {
						const params = {
							textDocument: {
								uri: model.uri.toString(),
							},
							position: toLspPosition(position),
						};
						const response = await this.sendRequest<LspCompletionResponse>(
							"textDocument/completion",
							params,
						);
						return this.convertCompletionResponse(response, model, position);
					} catch (error) {
						logDebug("[pg-lsp] completion failed:", error);
						return { suggestions: [] };
					}
				},
			},
		);
		disposables.push(completionProvider);

		const hoverProvider = monaco.languages.registerHoverProvider(
			PG_LANGUAGE_ID,
			{
				provideHover: async (model, position) => {
					try {
						const params = {
							textDocument: { uri: model.uri.toString() },
							position: toLspPosition(position),
						};
						const hover = await this.sendRequest<LspHover>(
							"textDocument/hover",
							params,
						);
						return this.convertHover(hover);
					} catch (error) {
						logDebug("[pg-lsp] hover failed:", error);
						return null;
					}
				},
			},
		);
		disposables.push(hoverProvider);

		const formattingProvider =
			monaco.languages.registerDocumentFormattingEditProvider(PG_LANGUAGE_ID, {
				provideDocumentFormattingEdits: async (model, _options, _token) => {
					try {
						// Build formatting options with custom formatter config
						const options: Record<string, unknown> = {
							tabSize: _options.tabSize,
							insertSpaces: _options.insertSpaces,
						};

						// Add custom formatter config if set
						// Filter out null/undefined values since LSP FormattingProperty
						// only supports bool, number, and string
						if (currentFormatterConfig) {
							for (const [key, value] of Object.entries(currentFormatterConfig)) {
								if (value !== null && value !== undefined) {
									options[key] = value;
								}
							}
						}

						const params = {
							textDocument: { uri: model.uri.toString() },
							options,
						};
						const edits = await this.sendRequest<LspTextEdit[]>(
							"textDocument/formatting",
							params,
						);
						if (!edits) return [];
						return edits.map((edit) => ({
							range: toMonacoRange(edit.range),
							text: edit.newText,
						}));
					} catch (error) {
						logDebug("[pg-lsp] formatting failed:", error);
						return [];
					}
				},
			});
		disposables.push(formattingProvider);

		return disposables;
	}

	private convertCompletionResponse(
		response: LspCompletionResponse,
		model: monaco.editor.ITextModel,
		position: monaco.Position,
	): monaco.languages.CompletionList {
		const items = Array.isArray(response) ? response : (response.items ?? []);

		const suggestions = items.map((item) =>
			this.toMonacoCompletion(item, model, position),
		);

		const isIncomplete = Array.isArray(response)
			? false
			: Boolean(response.isIncomplete);

		return {
			suggestions,
			incomplete: isIncomplete,
		};
	}

	private toMonacoCompletion(
		item: LspCompletionItem,
		model: monaco.editor.ITextModel,
		position: monaco.Position,
	): monaco.languages.CompletionItem {
		const range = item.textEdit
			? toMonacoRange(item.textEdit.range)
			: (() => {
					const word = model.getWordUntilPosition(position);
					return new monaco.Range(
						position.lineNumber,
						word.startColumn,
						position.lineNumber,
						word.endColumn,
					);
				})();

		const insertText = item.textEdit?.newText ?? item.insertText ?? item.label;
		const documentation = normalizeDocumentation(item.documentation);

		const insertTextRules =
			item.insertTextFormat === 2
				? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
				: undefined;

		return {
			label: item.label,
			kind: convertCompletionKind(item.kind),
			detail: item.detail ?? undefined,
			documentation,
			insertText,
			range,
			filterText: item.filterText,
			sortText: item.sortText,
			insertTextRules,
		};
	}

	private convertHover(hover: LspHover | null): monaco.languages.Hover | null {
		if (!hover) return null;

		const text = formatHoverContents(hover.contents);
		if (!text) {
			return null;
		}

		return {
			contents: [createMarkdownString(text)],
			range: hover.range ? toMonacoRange(hover.range) : undefined,
		};
	}
}

function toLspPosition(position: monaco.Position): LspPosition {
	return {
		line: position.lineNumber - 1,
		character: position.column - 1,
	};
}

function toMonacoRange(range: LspRange): monaco.IRange {
	return {
		startLineNumber: range.start.line + 1,
		endLineNumber: range.end.line + 1,
		startColumn: range.start.character + 1,
		endColumn: range.end.character + 1,
	};
}

function convertCompletionKind(
	kind?: number,
): monaco.languages.CompletionItemKind {
	if (typeof kind !== "number") {
		return monaco.languages.CompletionItemKind.Text;
	}

	const monacoKinds = monaco.languages.CompletionItemKind;
	const kindMap: Record<number, monaco.languages.CompletionItemKind> = {
		1: monacoKinds.Text,
		2: monacoKinds.Method,
		3: monacoKinds.Function,
		4: monacoKinds.Constructor,
		5: monacoKinds.Field,
		6: monacoKinds.Variable,
		7: monacoKinds.Class,
		8: monacoKinds.Interface,
		9: monacoKinds.Module,
		10: monacoKinds.Property,
		11: monacoKinds.Unit,
		12: monacoKinds.Value,
		13: monacoKinds.Enum,
		14: monacoKinds.Keyword,
		15: monacoKinds.Snippet,
		16: monacoKinds.Color,
		17: monacoKinds.File,
		18: monacoKinds.Reference,
		19: monacoKinds.Folder,
		20: monacoKinds.EnumMember,
		21: monacoKinds.Constant,
		22: monacoKinds.Struct,
		23: monacoKinds.Event,
		24: monacoKinds.Operator,
		25: monacoKinds.TypeParameter,
	};

	return kindMap[kind] ?? monacoKinds.Text;
}

function normalizeDocumentation(
	documentation?: LspCompletionItem["documentation"],
): string | monaco.IMarkdownString | undefined {
	if (!documentation) {
		return undefined;
	}

	if (typeof documentation === "string") {
		return documentation;
	}

	const value = documentation.value ?? "";
	if (!value) return undefined;

	const normalized =
		documentation.kind === "markdown"
			? value
			: ["```", value, "```"].join("\n");
	return createMarkdownString(normalized);
}

function formatHoverContents(contents: unknown): string {
	if (Array.isArray(contents)) {
		return contents.map(formatHoverContents).filter(Boolean).join("\n\n");
	}

	if (!contents) return "";

	if (typeof contents === "string") {
		return contents;
	}

	if (typeof contents === "object" && "value" in contents) {
		return String((contents as { value: string }).value);
	}

	if (
		typeof contents === "object" &&
		"language" in contents &&
		"value" in contents
	) {
		const { language, value } = contents as {
			language: string;
			value: string;
		};
		return ["```" + language, value, "```"].join("\n");
	}

	return "";
}

function logDebug(...args: unknown[]): void {
	if (typeof window === "undefined") {
		return;
	}
	// Debug logging is disabled by default
	// Set window.__OCTOFHIR_LSP_DEBUG__ = true to enable
	if (window.__OCTOFHIR_LSP_DEBUG__ === true) {
		// eslint-disable-next-line no-console
		console.debug(...args);
	}
}

function createMarkdownString(value: string): monaco.IMarkdownString {
	return { value, supportThemeIcons: true };
}

declare global {
	interface Window {
		MonacoEnvironment?: {
			getWorker?: (workerId: string, label: string) => Worker;
		};
		__OCTOFHIR_LSP_DEBUG__?: boolean;
	}
}
