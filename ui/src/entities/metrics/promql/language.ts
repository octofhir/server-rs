/**
 * PromQL for Monaco: highlighting, context-aware completion (metrics, labels, label values,
 * durations, functions, dashboard variables) and hovers. Completion data comes from the server's
 * metrics API, scoped to this deployment's series, and is cached for a minute.
 *
 * The completion contexts follow the approach of Prometheus' Apache-2.0 `codemirror-promql`.
 */

import { type MetricInfo, metricsApi } from "../api";
import { monaco } from "@/shared/monaco/config";
import { AGGREGATIONS, BUILTIN_VARIABLES, DURATIONS, FUNCTIONS, KEYWORDS, lookupFunction } from "./functions";

export const PROMQL_LANGUAGE_ID = "promql";

const CACHE_MS = 60_000;
const cache = new Map<string, { at: number; value: Promise<unknown> }>();

function cached<T>(key: string, load: () => Promise<T>): Promise<T> {
  const hit = cache.get(key);
  if (hit && Date.now() - hit.at < CACHE_MS) return hit.value as Promise<T>;
  const value = load().catch((error) => {
    cache.delete(key);
    throw error;
  });
  cache.set(key, { at: Date.now(), value });
  return value;
}

const catalog = () => cached("metrics", metricsApi.metrics);
const labelNames = (metric?: string) => cached(`labels:${metric ?? ""}`, () => metricsApi.labels(metric));
const labelValues = (label: string, metric?: string) =>
  cached(`values:${label}:${metric ?? ""}`, () => metricsApi.labelValues(label, metric));

/** Dashboard variables offered in each editor, by model URI. */
const modelVariables = new Map<string, string[]>();

export function setPromqlVariables(model: monaco.editor.ITextModel, names: string[]): void {
  modelVariables.set(model.uri.toString(), names);
}

export function clearPromqlVariables(model: monaco.editor.ITextModel): void {
  modelVariables.delete(model.uri.toString());
}

type Context =
  | { kind: "labelName"; metric?: string; prefix: string }
  | { kind: "labelValue"; metric?: string; label: string; prefix: string; quoted: boolean }
  | { kind: "duration"; prefix: string }
  | { kind: "grouping"; prefix: string }
  | { kind: "expression"; prefix: string };

const IDENT = /[a-zA-Z_:][a-zA-Z0-9_:]*$/;

/** Where the cursor is, read from the text before it. */
export function completionContext(before: string): Context {
  const prefix = /[$a-zA-Z0-9_:]*$/.exec(before)?.[0] ?? "";
  let depthBrace = -1;
  let depthBracket = -1;
  let parens: number[] = [];
  let inString: string | null = null;
  for (let index = 0; index < before.length; index++) {
    const ch = before[index];
    if (inString) {
      if (ch === "\\") index++;
      else if (ch === inString) inString = null;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") inString = ch;
    else if (ch === "{") depthBrace = index;
    else if (ch === "}") depthBrace = -1;
    else if (ch === "[") depthBracket = index;
    else if (ch === "]") depthBracket = -1;
    else if (ch === "(") parens.push(index);
    else if (ch === ")") parens = parens.slice(0, -1);
  }

  if (depthBrace >= 0) {
    const metric = IDENT.exec(before.slice(0, depthBrace))?.[0];
    const inside = before.slice(depthBrace + 1);
    const value = /([a-zA-Z_][a-zA-Z0-9_]*)\s*(=~|!~|!=|=)\s*(["'`]?)([^"'`]*)$/.exec(inside);
    if (value && (inString || value[3] === "")) {
      return { kind: "labelValue", metric, label: value[1], prefix: value[4], quoted: value[3] !== "" };
    }
    return { kind: "labelName", metric, prefix };
  }
  if (depthBracket >= 0) return { kind: "duration", prefix };
  const open = parens.at(-1);
  if (open != null && /\b(by|without|on|ignoring|group_left|group_right)\s*$/.test(before.slice(0, open))) {
    return { kind: "grouping", prefix };
  }
  return { kind: "expression", prefix };
}

function range(position: monaco.Position, prefix: string): monaco.IRange {
  return {
    startLineNumber: position.lineNumber,
    endLineNumber: position.lineNumber,
    startColumn: position.column - prefix.length,
    endColumn: position.column,
  };
}

function metricDoc(metric: MetricInfo): string {
  return [metric.type ? `**${metric.type}**` : null, metric.help].filter(Boolean).join(" — ");
}

let registered = false;

export function ensurePromqlLanguageRegistered(): void {
  if (registered) return;
  registered = true;
  const { languages } = monaco;
  const kinds = languages.CompletionItemKind;

  languages.register({ id: PROMQL_LANGUAGE_ID });
  languages.setLanguageConfiguration(PROMQL_LANGUAGE_ID, {
    comments: { lineComment: "#" },
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"', notIn: ["string"] },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
    ],
    wordPattern: /(-?\d*\.\d\w*)|([$a-zA-Z_:][a-zA-Z0-9_:]*)/g,
  });

  languages.setMonarchTokensProvider(PROMQL_LANGUAGE_ID, {
    functions: [...FUNCTIONS, ...AGGREGATIONS].map((item) => item.name),
    keywords: KEYWORDS.map((item) => item.name),
    tokenizer: {
      root: [
        [/#.*$/, "comment"],
        [/\$\{?[a-zA-Z_][a-zA-Z0-9_]*\}?/, "predefined"],
        [/\{/, { token: "delimiter.curly", next: "@matchers" }],
        [/\[/, { token: "delimiter.square", next: "@duration" }],
        [/"([^"\\]|\\.)*"/, "string"],
        [/'([^'\\]|\\.)*'/, "string"],
        [/`[^`]*`/, "string"],
        [/\d+(\.\d+)?(e[+-]?\d+)?(ms|s|m|h|d|w|y)?/, "number"],
        [
          /[a-zA-Z_:][a-zA-Z0-9_:]*/,
          { cases: { "@functions": "keyword.special", "@keywords": "keyword", "@default": "type" } },
        ],
        [/[-+*/%^=!<>]+|\b(and|or|unless)\b/, "operator"],
        [/[(),]/, "delimiter"],
      ],
      matchers: [
        [/\}/, { token: "delimiter.curly", next: "@pop" }],
        [/[a-zA-Z_][a-zA-Z0-9_]*/, "identifier"],
        [/=~|!~|!=|=/, "operator"],
        [/"([^"\\]|\\.)*"/, "string"],
        [/'([^'\\]|\\.)*'/, "string"],
        [/`[^`]*`/, "string"],
        [/,|\bor\b/, "delimiter"],
      ],
      duration: [
        [/\]/, { token: "delimiter.square", next: "@pop" }],
        [/\$\{?[a-zA-Z_][a-zA-Z0-9_]*\}?/, "predefined"],
        [/\d+(ms|s|m|h|d|w|y)/, "number"],
        [/:/, "delimiter"],
      ],
    },
  });

  languages.registerCompletionItemProvider(PROMQL_LANGUAGE_ID, {
    triggerCharacters: ["{", ",", '"', "=", "[", "(", "$", " "],
    async provideCompletionItems(model, position) {
      const before = model.getValueInRange({
        startLineNumber: 1,
        startColumn: 1,
        endLineNumber: position.lineNumber,
        endColumn: position.column,
      });
      const context = completionContext(before);
      const at = range(position, context.prefix);
      try {
        switch (context.kind) {
          case "labelName": {
            const names = await labelNames(context.metric);
            return {
              suggestions: names
                .filter((name) => name !== "__name__")
                .map((name) => ({
                  label: name,
                  kind: kinds.Field,
                  insertText: `${name}=`,
                  range: at,
                  command: { id: "editor.action.triggerSuggest", title: "values" },
                })),
            };
          }
          case "labelValue": {
            const values = await labelValues(context.label, context.metric);
            const variables = modelVariables.get(model.uri.toString()) ?? [];
            const valueRange = range(position, context.prefix);
            const wrap = (text: string) => (context.quoted ? text : `"${text}"`);
            return {
              suggestions: [
                ...variables.map((name) => ({
                  label: `$${name}`,
                  kind: kinds.Variable,
                  insertText: wrap(`$${name}`),
                  detail: "dashboard variable",
                  range: valueRange,
                  sortText: "0",
                })),
                ...values.map((value) => ({
                  label: value,
                  kind: kinds.Value,
                  insertText: wrap(value),
                  range: valueRange,
                  sortText: "1",
                })),
              ],
            };
          }
          case "duration":
            return {
              suggestions: DURATIONS.map((duration, index) => ({
                label: duration,
                kind: duration.startsWith("$") ? kinds.Variable : kinds.Unit,
                insertText: duration,
                range: at,
                sortText: String(index).padStart(2, "0"),
                documentation: BUILTIN_VARIABLES.find((v) => v.name === duration)?.doc,
              })),
            };
          case "grouping": {
            const names = await labelNames();
            return {
              suggestions: names
                .filter((name) => name !== "__name__")
                .map((name) => ({ label: name, kind: kinds.Field, insertText: name, range: at })),
            };
          }
          case "expression": {
            const metrics = await catalog().catch(() => [] as MetricInfo[]);
            const variables = modelVariables.get(model.uri.toString()) ?? [];
            return {
              suggestions: [
                ...metrics.map((metric) => ({
                  label: metric.name,
                  kind: kinds.Constant,
                  insertText: metric.name,
                  detail: metric.type ?? undefined,
                  documentation: metric.help ?? undefined,
                  range: at,
                  sortText: "1",
                })),
                ...[...AGGREGATIONS, ...FUNCTIONS].map((item) => ({
                  label: item.name,
                  kind: kinds.Function,
                  insertText: item.snippet,
                  insertTextRules: languages.CompletionItemInsertTextRule.InsertAsSnippet,
                  detail: item.signature,
                  documentation: item.doc,
                  range: at,
                  sortText: "2",
                })),
                ...KEYWORDS.map((item) => ({
                  label: item.name,
                  kind: kinds.Keyword,
                  insertText: item.name,
                  documentation: item.doc,
                  range: at,
                  sortText: "3",
                })),
                ...[...BUILTIN_VARIABLES.map((v) => v.name), ...variables.map((name) => `$${name}`)].map((name) => ({
                  label: name,
                  kind: kinds.Variable,
                  insertText: name,
                  range: at,
                  sortText: "4",
                })),
              ],
            };
          }
        }
      } catch {
        return { suggestions: [] };
      }
    },
  });

  languages.registerHoverProvider(PROMQL_LANGUAGE_ID, {
    async provideHover(model, position) {
      const word = model.getWordAtPosition(position);
      if (!word) return null;
      const hoverRange = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };
      const known = lookupFunction(word.word);
      if (known) {
        return { range: hoverRange, contents: [{ value: `\`${known.signature}\`` }, { value: known.doc }] };
      }
      const variable = BUILTIN_VARIABLES.find((v) => v.name === word.word);
      if (variable) return { range: hoverRange, contents: [{ value: `\`${variable.name}\`` }, { value: variable.doc }] };
      const metrics = await catalog().catch(() => [] as MetricInfo[]);
      const metric =
        metrics.find((m) => m.name === word.word) ??
        metrics.find((m) => m.name === word.word.replace(/_(bucket|sum|count)$/, ""));
      if (!metric) return null;
      return {
        range: hoverRange,
        contents: [{ value: `\`${word.word}\`` }, ...(metricDoc(metric) ? [{ value: metricDoc(metric) }] : [])],
      };
    },
  });
}
