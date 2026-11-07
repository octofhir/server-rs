/**
 * Client of the server's FML language server (`/api/lsp/fml`, the `fml-lsp` crate) for Monaco:
 * completion, hover, signature help, go to definition, references, rename, outline, formatting
 * and semantic tokens (the colours of variables, elements, groups, transforms and types).
 *
 * Problems keep coming from `/api/fml/check`, which the page also lists; the server publishes the
 * same ones over the socket, and they are not shown twice.
 */

import type * as Monaco from "monaco-editor";
import { monaco } from "@/shared/monaco/config";
import { FML_LANGUAGE_ID } from "./fmlLanguage";

type Position = { line: number; character: number };
type Range = { start: Position; end: Position };
type MarkupContent = { kind: string; value: string } | string;

interface CompletionItem {
  label: string;
  kind?: number;
  detail?: string;
  documentation?: MarkupContent;
  sortText?: string;
  insertText?: string;
  insertTextFormat?: number;
}

interface DocumentSymbol {
  name: string;
  detail?: string;
  kind: number;
  range: Range;
  selectionRange: Range;
  children?: DocumentSymbol[];
}

interface TextEdit {
  range: Range;
  newText: string;
}

interface Pending {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timeout: number;
}

const REQUEST_TIMEOUT_MS = 15_000;

function lspUrl(): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/lsp/fml`;
}

const toPosition = (position: Monaco.IPosition): Position => ({
  line: position.lineNumber - 1,
  character: position.column - 1,
});

const toRange = (range: Range): Monaco.IRange => ({
  startLineNumber: range.start.line + 1,
  startColumn: range.start.character + 1,
  endLineNumber: range.end.line + 1,
  endColumn: range.end.character + 1,
});

const markdown = (content?: MarkupContent): Monaco.IMarkdownString | undefined => {
  if (content === undefined) return undefined;
  return { value: typeof content === "string" ? content : content.value };
};

/** LSP CompletionItemKind (1-based) to Monaco's. */
function completionKind(kind?: number): Monaco.languages.CompletionItemKind {
  const k = monaco.languages.CompletionItemKind;
  switch (kind) {
    case 2:
      return k.Method;
    case 3:
      return k.Function;
    case 5:
      return k.Field;
    case 6:
      return k.Variable;
    case 7:
      return k.Class;
    case 14:
      return k.Keyword;
    case 21:
      return k.Constant;
    default:
      return k.Text;
  }
}

/** LSP SymbolKind (1-based) to Monaco's (0-based, same order). */
const symbolKind = (kind: number): Monaco.languages.SymbolKind =>
  (kind - 1) as Monaco.languages.SymbolKind;

function toSymbol(symbol: DocumentSymbol): Monaco.languages.DocumentSymbol {
  return {
    name: symbol.name,
    detail: symbol.detail ?? "",
    kind: symbolKind(symbol.kind),
    tags: [],
    range: toRange(symbol.range),
    selectionRange: toRange(symbol.selectionRange),
    children: symbol.children?.map(toSymbol),
  };
}

const toEdit = (edit: TextEdit): Monaco.languages.TextEdit => ({
  range: toRange(edit.range),
  text: edit.newText,
});

class FmlLspConnection {
  private socket: WebSocket | undefined;
  private ready: Promise<void> | undefined;
  private requestId = 0;
  private readonly pending = new Map<number, Pending>();
  private readonly models = new Map<string, Monaco.editor.ITextModel>();
  private readonly disposables: Monaco.IDisposable[] = [];
  private legend: Monaco.languages.SemanticTokensLegend = { tokenTypes: [], tokenModifiers: [] };
  private readonly tokensChanged = new monaco.Emitter<void>();
  private reconnectDelay = 1000;
  private disposed = false;

  constructor() {
    this.disposables.push(...this.registerProviders());
    this.connect();
  }

  dispose(): void {
    this.disposed = true;
    for (const disposable of this.disposables) disposable.dispose();
    this.tokensChanged.dispose();
    for (const pending of this.pending.values()) {
      window.clearTimeout(pending.timeout);
      pending.reject(new Error("FML language server closed"));
    }
    this.pending.clear();
    this.socket?.close();
  }

  /** Tracks a model: opened now, changes sent as they happen, closed with the returned function. */
  bind(model: Monaco.editor.ITextModel): () => void {
    const uri = model.uri.toString();
    this.models.set(uri, model);
    this.notifyOpen(model);
    let timer: number | undefined;
    const listener = model.onDidChangeContent(() => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        this.notify("textDocument/didChange", {
          textDocument: { uri, version: model.getVersionId() },
          contentChanges: [{ text: model.getValue() }],
        });
      }, 150);
    });
    return () => {
      window.clearTimeout(timer);
      listener.dispose();
      this.models.delete(uri);
      this.notify("textDocument/didClose", { textDocument: { uri } });
    };
  }

  private connect(): void {
    if (this.disposed) return;
    const socket = new WebSocket(lspUrl());
    this.socket = socket;
    this.ready = new Promise((resolve, reject) => {
      socket.onopen = () => {
        this.reconnectDelay = 1000;
        this.send("initialize", {
          processId: null,
          rootUri: null,
          capabilities: { textDocument: { completion: { completionItem: { snippetSupport: true } } } },
          clientInfo: { name: "octofhir-ui" },
        })
          .then((result) => {
            const legend = (result as { capabilities?: { semanticTokensProvider?: { legend?: Monaco.languages.SemanticTokensLegend } } })
              .capabilities?.semanticTokensProvider?.legend;
            if (legend) this.legend = legend;
            this.raw({ method: "initialized", params: {} });
            for (const model of this.models.values()) this.notifyOpen(model);
            this.tokensChanged.fire();
            resolve();
          })
          .catch(reject);
      };
    });
    // A connection that never opens is retried; nothing waits on it meanwhile.
    this.ready.catch(() => undefined);
    socket.onmessage = (event) => this.receive(String(event.data));
    socket.onclose = () => {
      this.socket = undefined;
      for (const pending of this.pending.values()) {
        window.clearTimeout(pending.timeout);
        pending.reject(new Error("FML language server disconnected"));
      }
      this.pending.clear();
      if (this.disposed) return;
      window.setTimeout(() => this.connect(), this.reconnectDelay);
      this.reconnectDelay = Math.min(this.reconnectDelay * 2, 30_000);
    };
  }

  private receive(text: string): void {
    let message: { id?: number; result?: unknown; error?: { message?: string } };
    try {
      message = JSON.parse(text);
    } catch {
      return;
    }
    if (message.id === undefined) return; // diagnostics: see the module comment
    const pending = this.pending.get(message.id);
    if (!pending) return;
    this.pending.delete(message.id);
    window.clearTimeout(pending.timeout);
    if (message.error) pending.reject(new Error(message.error.message ?? "FML language server error"));
    else pending.resolve(message.result);
  }

  private raw(message: Record<string, unknown>): boolean {
    if (this.socket?.readyState !== WebSocket.OPEN) return false;
    this.socket.send(JSON.stringify({ jsonrpc: "2.0", ...message }));
    return true;
  }

  private send(method: string, params: unknown): Promise<unknown> {
    const id = ++this.requestId;
    return new Promise((resolve, reject) => {
      if (!this.raw({ id, method, params })) {
        reject(new Error("FML language server is not connected"));
        return;
      }
      const timeout = window.setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${method} timed out`));
      }, REQUEST_TIMEOUT_MS);
      this.pending.set(id, { resolve, reject, timeout });
    });
  }

  private async request<T>(method: string, params: unknown): Promise<T | null> {
    try {
      await this.ready;
      return (await this.send(method, params)) as T | null;
    } catch {
      return null;
    }
  }

  private notify(method: string, params: unknown): void {
    this.raw({ method, params });
  }

  private notifyOpen(model: Monaco.editor.ITextModel): void {
    this.notify("textDocument/didOpen", {
      textDocument: {
        uri: model.uri.toString(),
        languageId: FML_LANGUAGE_ID,
        version: model.getVersionId(),
        text: model.getValue(),
      },
    });
  }

  private at(model: Monaco.editor.ITextModel, position: Monaco.IPosition) {
    return { textDocument: { uri: model.uri.toString() }, position: toPosition(position) };
  }

  private registerProviders(): Monaco.IDisposable[] {
    const languages = monaco.languages;
    const id = FML_LANGUAGE_ID;
    return [
      languages.registerCompletionItemProvider(id, {
        triggerCharacters: [".", ":"],
        provideCompletionItems: async (model, position) => {
          const list = await this.request<{ items: CompletionItem[] }>(
            "textDocument/completion",
            this.at(model, position),
          );
          const word = model.getWordUntilPosition(position);
          const range = {
            startLineNumber: position.lineNumber,
            endLineNumber: position.lineNumber,
            startColumn: word.startColumn,
            endColumn: word.endColumn,
          };
          return {
            suggestions: (list?.items ?? []).map((item) => ({
              label: item.label,
              kind: completionKind(item.kind),
              detail: item.detail,
              documentation: markdown(item.documentation),
              sortText: item.sortText,
              insertText: item.insertText ?? item.label,
              insertTextRules:
                item.insertTextFormat === 2
                  ? languages.CompletionItemInsertTextRule.InsertAsSnippet
                  : undefined,
              range,
            })),
          };
        },
      }),
      languages.registerHoverProvider(id, {
        provideHover: async (model, position) => {
          const hover = await this.request<{ contents: MarkupContent; range?: Range }>(
            "textDocument/hover",
            this.at(model, position),
          );
          if (!hover) return null;
          const contents = markdown(hover.contents);
          return {
            contents: contents ? [contents] : [],
            range: hover.range ? toRange(hover.range) : undefined,
          };
        },
      }),
      languages.registerSignatureHelpProvider(id, {
        signatureHelpTriggerCharacters: ["(", ","],
        provideSignatureHelp: async (model, position) => {
          const help = await this.request<{
            signatures: Array<{ label: string; documentation?: MarkupContent; parameters?: Array<{ label: string }> }>;
            activeSignature?: number;
            activeParameter?: number;
          }>("textDocument/signatureHelp", this.at(model, position));
          if (!help) return null;
          return {
            value: {
              signatures: help.signatures.map((signature) => ({
                label: signature.label,
                documentation: markdown(signature.documentation),
                parameters: (signature.parameters ?? []).map((parameter) => ({ label: parameter.label })),
              })),
              activeSignature: help.activeSignature ?? 0,
              activeParameter: help.activeParameter ?? 0,
            },
            dispose: () => undefined,
          };
        },
      }),
      languages.registerDefinitionProvider(id, {
        provideDefinition: async (model, position) => {
          const location = await this.request<{ uri: string; range: Range }>(
            "textDocument/definition",
            this.at(model, position),
          );
          return location ? { uri: model.uri, range: toRange(location.range) } : null;
        },
      }),
      languages.registerReferenceProvider(id, {
        provideReferences: async (model, position, context) => {
          const locations = await this.request<Array<{ range: Range }>>("textDocument/references", {
            ...this.at(model, position),
            context: { includeDeclaration: context.includeDeclaration },
          });
          return (locations ?? []).map((location) => ({ uri: model.uri, range: toRange(location.range) }));
        },
      }),
      languages.registerRenameProvider(id, {
        resolveRenameLocation: async (model, position) => {
          const range = await this.request<Range>("textDocument/prepareRename", this.at(model, position));
          if (!range) {
            return { range: new monaco.Range(1, 1, 1, 1), text: "", rejectReason: "Only variables and groups can be renamed" };
          }
          const monacoRange = toRange(range);
          return { range: monacoRange, text: model.getValueInRange(monacoRange) };
        },
        provideRenameEdits: async (model, position, newName) => {
          await this.ready;
          let edit: { changes?: Record<string, TextEdit[]> } | null;
          try {
            edit = (await this.send("textDocument/rename", { ...this.at(model, position), newName })) as typeof edit;
          } catch (error) {
            return { edits: [], rejectReason: error instanceof Error ? error.message : String(error) };
          }
          const edits = edit?.changes?.[model.uri.toString()] ?? [];
          return {
            edits: edits.map((textEdit) => ({
              resource: model.uri,
              textEdit: toEdit(textEdit),
              versionId: model.getVersionId(),
            })),
          };
        },
      }),
      languages.registerDocumentSymbolProvider(id, {
        provideDocumentSymbols: async (model) => {
          const symbols = await this.request<DocumentSymbol[]>("textDocument/documentSymbol", {
            textDocument: { uri: model.uri.toString() },
          });
          return (symbols ?? []).map(toSymbol);
        },
      }),
      languages.registerDocumentFormattingEditProvider(id, {
        provideDocumentFormattingEdits: async (model, options) => {
          const edits = await this.request<TextEdit[]>("textDocument/formatting", {
            textDocument: { uri: model.uri.toString() },
            options: { tabSize: options.tabSize, insertSpaces: options.insertSpaces },
          });
          return (edits ?? []).map(toEdit);
        },
      }),
      languages.registerDocumentSemanticTokensProvider(id, {
        onDidChange: this.tokensChanged.event,
        getLegend: () => this.legend,
        provideDocumentSemanticTokens: async (model) => {
          const tokens = await this.request<{ data: number[] }>("textDocument/semanticTokens/full", {
            textDocument: { uri: model.uri.toString() },
          });
          return tokens ? { data: new Uint32Array(tokens.data) } : null;
        },
        releaseDocumentSemanticTokens: () => undefined,
      }),
    ];
  }
}

let connection: FmlLspConnection | undefined;
let users = 0;

/**
 * Connects an editor model to the FML language server. The connection is shared by the editors
 * on the page and closes with the last of them.
 */
export function bindFmlModel(model: Monaco.editor.ITextModel): () => void {
  connection ??= new FmlLspConnection();
  users += 1;
  const unbind = connection.bind(model);
  return () => {
    unbind();
    users -= 1;
    if (users === 0) {
      connection?.dispose();
      connection = undefined;
    }
  };
}
