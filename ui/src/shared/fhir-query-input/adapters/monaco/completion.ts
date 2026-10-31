import type * as Monaco from "monaco-editor";
import { getCursorContext } from "../../core/cursor-context";
import { getSuggestions } from "../../core/suggestions";
import type { QueryInputMetadata, QuerySuggestion } from "../../core/types";
import {
  resolveResourceIdSuggestions,
  resolveValueSuggestions,
} from "../../services/value-sources";
import { LANGUAGE_ID } from "./language";

const SUGGESTION_KIND_MAP: Record<string, number> = {
  resource: 6, // Class
  operation: 1, // Method
  param: 4, // Field
  modifier: 11, // Keyword
  value: 12, // Value
  prefix: 14, // Operator
  special: 4, // Field
  "api-endpoint": 17, // File
  structural: 15, // Snippet
};

export function createCompletionProvider(
  getMetadata: () => QueryInputMetadata,
  basePath = "/fhir"
): Monaco.languages.CompletionItemProvider {
  const DYNAMIC_VARS = ["$guid", "$now", "$isoTimestamp", "$timestamp", "$randomInt"];

  return {
    triggerCharacters: ["/", "?", "&", ":", "=", "$", ",", ".", "{"],

    provideCompletionItems(
      model: Monaco.editor.ITextModel,
      position: Monaco.Position
    ): Monaco.languages.ProviderResult<Monaco.languages.CompletionList> {
      const lineContent = model.getLineContent(position.lineNumber);
      const cursorOffset = position.column - 1;
      const metadata = getMetadata();

      // {{variable}} completion — works in any position (URL, body, …).
      const before = lineContent.slice(0, cursorOffset);
      const varMatch = before.match(/\{\{\s*([^}{]*)$/);
      if (varMatch) {
        const frag = varMatch[1];
        const names = [...(metadata.variables ?? []), ...DYNAMIC_VARS];
        const range: Monaco.IRange = {
          startLineNumber: position.lineNumber,
          startColumn: position.column - frag.length,
          endLineNumber: position.lineNumber,
          endColumn: position.column,
        };
        return {
          suggestions: names.map((name, i) => ({
            label: name,
            kind: 12, // Value
            insertText: `${name}}}`,
            detail: name.startsWith("$") ? "dynamic" : "environment variable",
            range,
            sortText: `${name.startsWith("$") ? "1" : "0"}${String(i).padStart(3, "0")}`,
          })),
        };
      }

      const context = getCursorContext(lineContent, cursorOffset, metadata.resourceTypes, basePath);

      const suggestions = getSuggestions(context, metadata);

      const word = model.getWordUntilPosition(position);
      const range: Monaco.IRange = {
        startLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endLineNumber: position.lineNumber,
        endColumn: position.column,
      };

      // For some context types, we want to replace the fragment, not just the word
      const fragmentRange = getFragmentRange(
        lineContent,
        cursorOffset,
        context.type,
        position.lineNumber
      );

      const itemRange = fragmentRange ?? range;
      const items = suggestions.map((s, i) => toMonacoItem(s, itemRange, i));
      // Lists are capped and pre-filtered by the fragment, so Monaco must ask again on every
      // keystroke instead of filtering a stale first page (`Patient.na` would miss `name`).
      // While an id is being typed (`Patient/ann`), the id segment is what a pick replaces.
      const typingId = context.type === "next-after-id";
      const live =
        context.type === "query-value"
          ? resolveValueSuggestions(context, metadata)
          : context.type === "resource-id" || typingId
            ? resolveResourceIdSuggestions(
                typingId ? { ...context, fragment: context.resourceId ?? "" } : context,
                metadata
              )
            : null;
      if (!live) return { suggestions: items, incomplete: true };
      const liveRange = typingId
        ? { ...itemRange, startColumn: before.lastIndexOf("/") + 2 }
        : itemRange;

      // Codes and existing resources come from the server and lead; static ones follow.
      return live.then(
        (found) => ({
          suggestions: [
            ...found.map((s, i) => ({ ...toMonacoItem(s, liveRange, i), sortText: sortKey(0, i) })),
            ...items.map((item, i) => ({ ...item, sortText: sortKey(1, found.length + i) })),
          ],
          incomplete: true,
        }),
        () => ({ suggestions: items, incomplete: true })
      );
    },
  };
}

function toMonacoItem(
  suggestion: QuerySuggestion,
  range: Monaco.IRange,
  index: number
): Monaco.languages.CompletionItem {
  return {
    label: suggestion.description
      ? { label: suggestion.label, description: suggestion.description }
      : suggestion.label,
    kind: SUGGESTION_KIND_MAP[suggestion.kind] ?? 18, // Text
    insertText: suggestion.insertText,
    filterText: suggestion.filterText,
    detail: suggestion.detail,
    documentation: suggestion.documentation,
    range,
    sortText: sortKey(suggestion.sortPriority, index),
    // `name=`, `subject:Patient.` and `Patient/` expect more: keep the popup going.
    command: CONTINUES.test(suggestion.insertText)
      ? { id: "editor.action.triggerSuggest", title: "Suggest" }
      : undefined,
  };
}

const CONTINUES = /[=.:/?&]$/;

function sortKey(priority: number, index: number): string {
  return String(priority).padStart(3, "0") + String(index).padStart(4, "0");
}

function getFragmentRange(
  lineContent: string,
  cursorOffset: number,
  contextType: string,
  lineNumber: number
): Monaco.IRange | null {
  const before = lineContent.slice(0, cursorOffset);

  // Root: replace entire input so far (e.g. "/" → "/fhir")
  if (contextType === "root") {
    return {
      startLineNumber: lineNumber,
      startColumn: 1,
      endLineNumber: lineNumber,
      endColumn: cursorOffset + 1,
    };
  }

  // Query param name: replace from after last "?" or "&"
  if (contextType === "query-param") {
    const lastSep = Math.max(before.lastIndexOf("&"), before.lastIndexOf("?"));
    if (lastSep >= 0) {
      return {
        startLineNumber: lineNumber,
        startColumn: lastSep + 2,
        endLineNumber: lineNumber,
        endColumn: cursorOffset + 1,
      };
    }
  }

  // Query modifier: replace text AFTER the ":" (colon stays, insertText is just the modifier code)
  if (contextType === "query-modifier") {
    const lastColon = before.lastIndexOf(":");
    if (lastColon >= 0) {
      return {
        startLineNumber: lineNumber,
        startColumn: lastColon + 2, // position after the colon
        endLineNumber: lineNumber,
        endColumn: cursorOffset + 1,
      };
    }
  }

  // Query value: replace the value under the cursor (after "=", or after "," between OR-ed values)
  if (contextType === "query-value") {
    const valueStart = Math.max(before.lastIndexOf("="), before.lastIndexOf(","));
    if (valueStart >= 0) {
      return {
        startLineNumber: lineNumber,
        startColumn: valueStart + 2,
        endLineNumber: lineNumber,
        endColumn: cursorOffset + 1,
      };
    }
  }

  // Resource type or id: replace from after last "/"
  if (contextType === "resource-type" || contextType === "resource-id") {
    const lastSlash = before.lastIndexOf("/");
    if (lastSlash >= 0) {
      return {
        startLineNumber: lineNumber,
        startColumn: lastSlash + 2,
        endLineNumber: lineNumber,
        endColumn: cursorOffset + 1,
      };
    }
  }

  // Operations: replace from after last "/" (covers "$opname" fragment)
  if (
    contextType === "type-operation" ||
    contextType === "system-operation" ||
    contextType === "instance-operation"
  ) {
    const lastSlash = before.lastIndexOf("/");
    if (lastSlash >= 0) {
      return {
        startLineNumber: lineNumber,
        startColumn: lastSlash + 2,
        endLineNumber: lineNumber,
        endColumn: cursorOffset + 1,
      };
    }
  }

  // After resource/id: zero-width range at cursor (insert, don't replace)
  if (contextType === "next-after-resource" || contextType === "next-after-id") {
    return {
      startLineNumber: lineNumber,
      startColumn: cursorOffset + 1,
      endLineNumber: lineNumber,
      endColumn: cursorOffset + 1,
    };
  }

  return null;
}

export function registerCompletionProvider(
  monaco: typeof import("monaco-editor"),
  getMetadata: () => QueryInputMetadata,
  basePath?: string
): Monaco.IDisposable {
  return monaco.languages.registerCompletionItemProvider(
    LANGUAGE_ID,
    createCompletionProvider(getMetadata, basePath)
  );
}
