/**
 * FHIRPath LSP Client for Monaco Editor integration
 *
 * Provides LSP capabilities for FHIRPath expression editing including:
 * - Autocompletion for properties, functions, keywords, and constants
 * - Diagnostics with error recovery
 * - Context API for enhanced features (ViewDefinition, SQL on FHIR)
 */
import * as monaco from "monaco-editor";

const DEFAULT_FHIRPATH_LSP_PATH = "/api/lsp/fhirpath";
const textDecoder = new TextDecoder();

let fhirpathLanguageRegistered = false;
let activeConnection: FhirPathLspConnection | undefined;

export const FHIRPATH_LANGUAGE_ID = "fhirpath";

/**
 * Constant info for FHIRPath context
 */
export interface ConstantInfo {
	typeName: string;
	description?: string;
}

/**
 * Context parameters for FHIRPath LSP
 */
export interface SetContextParams {
	resourceType?: string;
	constants?: Record<string, ConstantInfo>;
	/** Path context for nested expressions (e.g., ["name"] means we're inside a forEach on name) */
	forEachContext?: string[];
}

/**
 * Register the FHIRPath language with Monaco
 */
export function ensureFhirPathLanguageRegistered(): void {
	const existingLanguages = monaco.languages.getLanguages();
	const alreadyRegistered = existingLanguages.some(
		(l) => l.id === FHIRPATH_LANGUAGE_ID,
	);

	if (fhirpathLanguageRegistered || alreadyRegistered) {
		fhirpathLanguageRegistered = true;
		return;
	}

	monaco.languages.register({
		id: FHIRPATH_LANGUAGE_ID,
		extensions: [".fhirpath"],
		aliases: ["FHIRPath"],
	});

	monaco.languages.setLanguageConfiguration(FHIRPATH_LANGUAGE_ID, {
		comments: {
			// FHIRPath doesn't have comments, but we need to provide something
		},
		brackets: [
			["(", ")"],
			["[", "]"],
		],
		autoClosingPairs: [
			{ open: "(", close: ")" },
			{ open: "[", close: "]" },
			{ open: "'", close: "'", notIn: ["string"] },
		],
		surroundingPairs: [
			{ open: "(", close: ")" },
			{ open: "[", close: "]" },
			{ open: "'", close: "'" },
		],
	});

	// Monarch tokenizer for FHIRPath syntax highlighting
	monaco.languages.setMonarchTokensProvider(FHIRPATH_LANGUAGE_ID, {
		defaultToken: "",
		tokenPostfix: ".fhirpath",

		keywords: [
			"and",
			"or",
			"xor",
			"implies",
			"not",
			"is",
			"as",
			"true",
			"false",
			"in",
			"contains",
			"div",
			"mod",
		],

		builtinFunctions: [
			"where",
			"select",
			"repeat",
			"ofType",
			"empty",
			"exists",
			"all",
			"allTrue",
			"anyTrue",
			"allFalse",
			"anyFalse",
			"subsetOf",
			"supersetOf",
			"count",
			"distinct",
			"isDistinct",
			"first",
			"last",
			"tail",
			"skip",
			"take",
			"single",
			"iif",
			"toBoolean",
			"convertsToBoolean",
			"toInteger",
			"convertsToInteger",
			"toDate",
			"convertsToDate",
			"toDateTime",
			"convertsToDateTime",
			"toDecimal",
			"convertsToDecimal",
			"toQuantity",
			"convertsToQuantity",
			"toString",
			"convertsToString",
			"toTime",
			"convertsToTime",
			"indexOf",
			"substring",
			"startsWith",
			"endsWith",
			"contains",
			"upper",
			"lower",
			"replace",
			"matches",
			"replaceMatches",
			"length",
			"children",
			"descendants",
			"trace",
			"now",
			"timeOfDay",
			"today",
			"hasValue",
			"getValue",
			"combine",
			"union",
			"intersect",
			"exclude",
			"aggregate",
			"resolve",
			"extension",
			"memberOf",
		],

		operators: [
			"=",
			"~",
			"!=",
			"!~",
			">",
			"<",
			"<=",
			">=",
			"+",
			"-",
			"*",
			"/",
			"|",
			"&",
		],

		symbols: /[=><!~?:&|+\-*/^%]+/,

		tokenizer: {
			root: [
				// Identifiers and keywords
				[
					/[a-zA-Z_]\w*/,
					{
						cases: {
							"@keywords": "keyword",
							"@builtinFunctions": "predefined",
							"@default": "identifier",
						},
					},
				],

				// External constants (%name)
				[/%[a-zA-Z_]\w*/, "constant"],

				// Keywords ($this, $index, $total)
				[/\$[a-zA-Z_]\w*/, "keyword.special"],

				// Whitespace
				{ include: "@whitespace" },

				// Delimiters and operators
				[/[{}()[\]]/, "@brackets"],
				[/@symbols/, { cases: { "@operators": "operator", "@default": "" } }],

				// Numbers
				[/\d*\.\d+([eE][-+]?\d+)?/, "number.float"],
				[/\d+/, "number"],

				// Strings
				[/'([^'\\]|\\.)*$/, "string.invalid"], // non-terminated
				[/'/, { token: "string.quote", bracket: "@open", next: "@string" }],
			],

			string: [
				[/[^\\']+/, "string"],
				[/\\./, "string.escape"],
				[/'/, { token: "string.quote", bracket: "@close", next: "@pop" }],
			],

			whitespace: [[/[ \t\r\n]+/, "white"]],
		},
	});

	fhirpathLanguageRegistered = true;
}

/**
 * Build the WebSocket URL for the FHIRPath LSP server
 */
export function buildFhirPathLspUrl(pathOverride?: string): string {
	const path = pathOverride || DEFAULT_FHIRPATH_LSP_PATH;
	const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
	return `${protocol}//${window.location.host}${path}`;
}

/**
 * Start the FHIRPath LSP connection
 */
export async function startFhirPathLsp(
	getUrl: () => string,
): Promise<() => void> {
	if (activeConnection) {
		return () => stopFhirPathLsp();
	}

	const connection = new FhirPathLspConnection(getUrl);
	activeConnection = connection;

	try {
		await connection.start();
	} catch (error) {
		connection.dispose();
		activeConnection = undefined;
		throw error;
	}

	return () => stopFhirPathLsp();
}

/**
 * Stop the FHIRPath LSP connection
 */
export function stopFhirPathLsp(): void {
	activeConnection?.dispose();
	activeConnection = undefined;
}

/**
 * Bind a Monaco model to the FHIRPath LSP
 */
export function bindFhirPathModelToLsp(
	model: monaco.editor.ITextModel,
): () => void {
	if (!activeConnection) {
		return () => {};
	}
	return activeConnection.bindModel(model);
}

/**
 * Set the FHIRPath context (resource type and constants)
 */
export function setFhirPathContext(params: SetContextParams): void {
	activeConnection?.sendContextNotification(params);
}

/**
 * Clear the FHIRPath context
 */
export function clearFhirPathContext(): void {
	activeConnection?.sendClearContextNotification();
}

/**
 * Check if FHIRPath LSP is connected
 */
export function isFhirPathLspConnected(): boolean {
	return activeConnection?.isConnected() ?? false;
}

// ============================================================================
// LSP Types
// ============================================================================

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
	didChangeDebounce?: ReturnType<typeof setTimeout>;
}

// ============================================================================
// FHIRPath LSP Connection
// ============================================================================

class FhirPathLspConnection {
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

	public isConnected(): boolean {
		return this.initialized && this.socket?.readyState === WebSocket.OPEN;
	}

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

		this.rejectAllPendingRequests(
			new Error("FHIRPath LSP connection disposed"),
		);

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

		const tracked: TrackedDocument = {
			uri,
			model,
			listener: { dispose: () => {} }, // placeholder, set below
		};

		// Debounce didChange notifications to reduce LSP traffic
		const listener = model.onDidChangeContent(() => {
			if (tracked.didChangeDebounce) {
				clearTimeout(tracked.didChangeDebounce);
			}
			tracked.didChangeDebounce = setTimeout(() => {
				this.sendDidChange(model);
			}, 100); // 100ms debounce for LSP updates
		});
		tracked.listener = listener;

		this.documents.set(uri, tracked);
		this.sendDidOpen(model);

		return () => {
			if (tracked.didChangeDebounce) {
				clearTimeout(tracked.didChangeDebounce);
			}
			listener.dispose();
			this.documents.delete(uri);
			this.sendNotification("textDocument/didClose", {
				textDocument: { uri },
			});
		};
	}

	public sendContextNotification(params: SetContextParams): void {
		this.sendNotification("fhirpath/setContext", params);
	}

	public sendClearContextNotification(): void {
		this.sendNotification("fhirpath/clearContext", {});
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
			logDebug("[fhirpath-lsp] socket error", event);
		};

		socket.onclose = (event) => {
			logDebug(
				`[fhirpath-lsp] socket closed ${event.code} ${event.reason || ""}`.trim(),
			);
			this.initialized = false;
			this.socket = undefined;
			this.notificationQueue.length = 0;
			if (this.readyReject) {
				this.readyReject(
					new Error(
						`FHIRPath LSP connection closed before ready (${event.code})`,
					),
				);
			}
			this.readyPromise = null;
			this.readyResolve = undefined;
			this.readyReject = undefined;
			this.rejectAllPendingRequests(
				new Error(`FHIRPath LSP socket closed (${event.code})`),
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
		logDebug(`[fhirpath-lsp] reconnecting in ${delay}ms`);

		this.reconnectHandle = window.setTimeout(() => {
			this.reconnectHandle = undefined;
			this.connect().catch((error) => {
				logDebug("[fhirpath-lsp] reconnect failed", error);
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
					logDebug("[fhirpath-lsp] failed to read blob message", error),
				);
			return;
		}

		const text =
			typeof data === "string" ? data : textDecoder.decode(data as ArrayBuffer);

		logDebug("[fhirpath-lsp] <= ", text);

		let message: Record<string, unknown>;
		try {
			message = JSON.parse(text);
		} catch (error) {
			logDebug("[fhirpath-lsp] failed to parse message", error);
			return;
		}

		if (typeof message.id !== "undefined") {
			const pending = this.pendingRequests.get(Number(message.id));
			if (!pending) return;

			this.pendingRequests.delete(Number(message.id));
			clearTimeout(pending.timeoutId);

			if (message.error) {
				const err = message.error as { message?: string };
				const msg =
					err.message ?? `FHIRPath LSP request ${pending.method} failed`;
				pending.reject(new Error(msg));
			} else {
				pending.resolve(message.result);
			}
			return;
		}

		if (message.method === "window/logMessage") {
			const params = (message.params ?? {}) as { message?: string };
			logDebug(`[fhirpath-lsp] server: ${params.message ?? "log message"}`);
			return;
		}

		if (message.method === "textDocument/publishDiagnostics") {
			this.handleDiagnostics(message.params as LspPublishDiagnosticsParams);
			return;
		}
	}

	private handleDiagnostics(params: LspPublishDiagnosticsParams): void {
		if (!params || !params.uri) {
			return;
		}

		const uri = params.uri;
		const diagnostics = params.diagnostics || [];

		const doc = this.documents.get(uri);
		if (!doc || doc.model.isDisposed()) {
			return;
		}

		const markers = diagnostics.map((diag) => {
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
				source: diag.source || "fhirpath",
				code: diag.code ? String(diag.code) : undefined,
			};
		});

		monaco.editor.setModelMarkers(doc.model, "fhirpath-lsp", markers);
	}

	private async sendRequest<T>(
		method: string,
		params?: unknown,
		allowDuringInit = false,
	): Promise<T> {
		if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
			throw new Error("FHIRPath LSP socket is not ready");
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

		logDebug("[fhirpath-lsp] => ", payload);
		this.socket.send(payload);

		return new Promise<T>((resolve, reject) => {
			const timeoutId = window.setTimeout(() => {
				this.pendingRequests.delete(id);
				reject(
					new Error(`FHIRPath LSP request ${method} timed out after 15s`),
				);
			}, 15000);

			this.pendingRequests.set(id, {
				resolve: resolve as (value: unknown) => void,
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
		logDebug("[fhirpath-lsp] => ", payload);
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
				languageId: FHIRPATH_LANGUAGE_ID,
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
			FHIRPATH_LANGUAGE_ID,
			{
				triggerCharacters: [".", " ", "(", "%", "$"],
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
						logDebug("[fhirpath-lsp] completion failed:", error);
						return { suggestions: [] };
					}
				},
			},
		);
		disposables.push(completionProvider);

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
}

// ============================================================================
// Helper Functions
// ============================================================================

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
	return { value: normalized, supportThemeIcons: true };
}

function logDebug(...args: unknown[]): void {
	if (typeof window === "undefined") {
		return;
	}
	if (window.__OCTOFHIR_LSP_DEBUG__ === true) {
		console.debug(...args);
	}
}

declare global {
	interface Window {
		__OCTOFHIR_LSP_DEBUG__?: boolean;
	}
}
