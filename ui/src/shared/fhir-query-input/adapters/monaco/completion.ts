import type * as Monaco from "monaco-editor";
import { getCursorContext } from "../../core/cursor-context";
import { getSuggestions } from "../../core/suggestions";
import type { QueryInputMetadata, QuerySuggestion } from "../../core/types";
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
	basePath = "/fhir",
): Monaco.languages.CompletionItemProvider {
	return {
		triggerCharacters: ["/", "?", "&", ":", "=", "$", ",", "."],

		provideCompletionItems(
			model: Monaco.editor.ITextModel,
			position: Monaco.Position,
		): Monaco.languages.CompletionList {
			const lineContent = model.getLineContent(position.lineNumber);
			const cursorOffset = position.column - 1;
			const metadata = getMetadata();

			const context = getCursorContext(
				lineContent,
				cursorOffset,
				metadata.resourceTypes,
				basePath,
			);

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
				position.lineNumber,
			);

			return {
				suggestions: suggestions.map((s, i) =>
					toMonacoItem(s, fragmentRange ?? range, i),
				),
			};
		},
	};
}

function toMonacoItem(
	suggestion: QuerySuggestion,
	range: Monaco.IRange,
	index: number,
): Monaco.languages.CompletionItem {
	return {
		label: suggestion.label,
		kind: SUGGESTION_KIND_MAP[suggestion.kind] ?? 18, // Text
		insertText: suggestion.insertText,
		filterText: suggestion.filterText,
		detail: suggestion.detail,
		documentation: suggestion.documentation,
		range,
		sortText: String(suggestion.sortPriority).padStart(3, "0") + String(index).padStart(4, "0"),
	};
}

function getFragmentRange(
	lineContent: string,
	cursorOffset: number,
	contextType: string,
	lineNumber: number,
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

	// Query value: replace from after "="
	if (contextType === "query-value") {
		const lastEquals = before.lastIndexOf("=");
		if (lastEquals >= 0) {
			return {
				startLineNumber: lineNumber,
				startColumn: lastEquals + 2,
				endLineNumber: lineNumber,
				endColumn: cursorOffset + 1,
			};
		}
	}

	// Resource type: replace from after last "/"
	if (contextType === "resource-type") {
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
	basePath?: string,
): Monaco.IDisposable {
	return monaco.languages.registerCompletionItemProvider(
		LANGUAGE_ID,
		createCompletionProvider(getMetadata, basePath),
	);
}
