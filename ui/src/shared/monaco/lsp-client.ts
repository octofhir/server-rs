/**
 * PostgreSQL LSP client for Monaco editor.
 *
 * Connects to the OctoFHIR backend PostgreSQL LSP server via WebSocket.
 */

import * as monaco from "monaco-editor";
import { MonacoLanguageClient } from "monaco-languageclient";
import type { MessageTransports } from "vscode-languageclient";
import {
	toSocket,
	WebSocketMessageReader,
	WebSocketMessageWriter,
} from "vscode-ws-jsonrpc";

let languageClient: MonacoLanguageClient | null = null;
let webSocket: WebSocket | null = null;
let isConnecting = false;

/**
 * Get the authentication token from storage or current session.
 */
function getAuthToken(): string | null {
	// Try localStorage first (persistent login)
	const storedToken = localStorage.getItem("octofhir_token");
	if (storedToken) return storedToken;

	// Try sessionStorage (session login)
	const sessionToken = sessionStorage.getItem("octofhir_token");
	if (sessionToken) return sessionToken;

	return null;
}

/**
 * Build the WebSocket URL for the LSP endpoint.
 */
function buildLspUrl(): string {
	const token = getAuthToken();
	const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
	const host = window.location.host;

	let url = `${protocol}//${host}/api/pg-lsp`;
	if (token) {
		url += `?token=${encodeURIComponent(token)}`;
	}

	return url;
}

/**
 * Create WebSocket and message transports.
 */
function createWebSocket(url: string) {
	return new Promise((resolve, reject) => {
		const socket = new WebSocket(url);

		socket.onopen = () => {
			const socketWrapper = toSocket(socket);
			const reader = new WebSocketMessageReader(socketWrapper);
			const writer = new WebSocketMessageWriter(socketWrapper);
			webSocket = socket;
			resolve({ reader, writer });
		};

		socket.onerror = (event) => {
			console.error("[LSP] WebSocket error:", event);
			reject(new Error("Failed to connect to LSP server"));
		};

		socket.onclose = (event) => {
			console.log("[LSP] WebSocket closed:", event.code, event.reason);
			webSocket = null;
			languageClient = null;
		};
	});
}

/**
 * Initialize the LSP client and connect to the server.
 */
export async function initializeLspClient(): Promise<void> {
	// Prevent multiple concurrent connection attempts
	if (isConnecting || languageClient) {
		return;
	}

	isConnecting = true;

	try {
		const url = buildLspUrl();
		console.log("[LSP] Connecting to:", url.replace(/token=.*/, "token=***"));

		const transports = await createWebSocket(url);

		languageClient = new MonacoLanguageClient({
			name: "PostgreSQL LSP Client",
			clientOptions: {
				documentSelector: [{ language: "pgsql" }],
				errorHandler: {
					error: () => ({ action: 1 }),
					closed: () => ({ action: 2 }),
				},
			},
			messageTransports: transports as MessageTransports,
		});

		await languageClient.start();
		console.log("[LSP] Client started successfully");
	} catch (error) {
		console.error("[LSP] Failed to initialize client:", error);
		languageClient = null;
	} finally {
		isConnecting = false;
	}
}

/**
 * Disconnect the LSP client.
 */
export async function disconnectLspClient(): Promise<void> {
	if (languageClient) {
		await languageClient.stop();
		languageClient = null;
	}

	if (webSocket) {
		webSocket.close();
		webSocket = null;
	}
}

/**
 * Check if the LSP client is connected.
 */
export function isLspConnected(): boolean {
	return (
		languageClient !== null &&
		webSocket !== null &&
		webSocket.readyState === WebSocket.OPEN
	);
}

/**
 * Get or create a Monaco text model for LSP.
 * This ensures the model is properly registered with the language client.
 */
export function getOrCreateLspModel(
	uri: string,
	content: string,
): monaco.editor.ITextModel {
	const monacoUri = monaco.Uri.parse(uri);
	let model = monaco.editor.getModel(monacoUri);

	if (!model) {
		model = monaco.editor.createModel(content, "pgsql", monacoUri);
	} else if (model.getValue() !== content) {
		model.setValue(content);
	}

	return model;
}

/**
 * Register PostgreSQL language with Monaco if not already registered.
 */
export function registerPgsqlLanguage(): void {
	const languages = monaco.languages.getLanguages();
	const hasPgsql = languages.some((lang) => lang.id === "pgsql");

	if (!hasPgsql) {
		// Register basic pgsql language
		monaco.languages.register({
			id: "pgsql",
			extensions: [".sql", ".pgsql"],
			aliases: ["PostgreSQL", "pgsql", "postgres"],
			mimetypes: ["application/x-pgsql"],
		});

		// Set basic language configuration
		monaco.languages.setLanguageConfiguration("pgsql", {
			comments: {
				lineComment: "--",
				blockComment: ["/*", "*/"],
			},
			brackets: [
				["(", ")"],
				["[", "]"],
			],
			autoClosingPairs: [
				{ open: "(", close: ")" },
				{ open: "[", close: "]" },
				{ open: "'", close: "'", notIn: ["string"] },
				{ open: '"', close: '"', notIn: ["string"] },
			],
			surroundingPairs: [
				{ open: "(", close: ")" },
				{ open: "[", close: "]" },
				{ open: "'", close: "'" },
				{ open: '"', close: '"' },
			],
		});

		// Set basic token colors for SQL keywords
		monaco.languages.setMonarchTokensProvider("pgsql", {
			ignoreCase: true,
			keywords: [
				"SELECT",
				"FROM",
				"WHERE",
				"JOIN",
				"LEFT",
				"RIGHT",
				"INNER",
				"OUTER",
				"ON",
				"AS",
				"AND",
				"OR",
				"NOT",
				"IN",
				"BETWEEN",
				"LIKE",
				"IS",
				"NULL",
				"TRUE",
				"FALSE",
				"ORDER",
				"BY",
				"ASC",
				"DESC",
				"LIMIT",
				"OFFSET",
				"GROUP",
				"HAVING",
				"DISTINCT",
				"UNION",
				"INTERSECT",
				"EXCEPT",
				"INSERT",
				"INTO",
				"VALUES",
				"UPDATE",
				"SET",
				"DELETE",
				"CREATE",
				"ALTER",
				"DROP",
				"TABLE",
				"INDEX",
				"VIEW",
				"FUNCTION",
				"TRIGGER",
				"WITH",
				"RECURSIVE",
				"CASE",
				"WHEN",
				"THEN",
				"ELSE",
				"END",
				"CAST",
				"COALESCE",
				"NULLIF",
				"EXISTS",
				"ANY",
				"ALL",
			],
			operators: [
				"=",
				"<>",
				"!=",
				"<",
				"<=",
				">",
				">=",
				"+",
				"-",
				"*",
				"/",
				"%",
				"||",
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
			],
			tokenizer: {
				root: [
					[/--.*$/, "comment"],
					[/\/\*/, "comment", "@comment"],
					[
						/[a-zA-Z_]\w*/,
						{
							cases: {
								"@keywords": "keyword",
								"@default": "identifier",
							},
						},
					],
					[/'[^']*'/, "string"],
					[/"[^"]*"/, "string.identifier"],
					[/\d+(\.\d+)?/, "number"],
					[/[+\-*/<>=!@#$%^&|~?]+/, "operator"],
					[/[(),;[\]]/, "delimiter"],
				],
				comment: [
					[/[^/*]+/, "comment"],
					[/\*\//, "comment", "@pop"],
					[/[/*]/, "comment"],
				],
			},
		});
	}
}
