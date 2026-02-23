import { useState, useMemo, useCallback } from "react";
import { parseQueryAst } from "../core/parser";
import { getCursorContext } from "../core/cursor-context";
import { getSuggestions } from "../core/suggestions";
import { computeDiagnostics } from "../core/diagnostics";
import type { QueryAst, CursorContext, QuerySuggestion, QueryInputMetadata, Diagnostic } from "../core/types";

export interface UseFhirQueryInputOptions {
	metadata: QueryInputMetadata;
	basePath?: string;
	initialRaw?: string;
}

export interface UseFhirQueryInputReturn {
	raw: string;
	setRaw: (newRaw: string, newCursor: number) => void;
	cursorOffset: number;
	ast: QueryAst;
	context: CursorContext;
	suggestions: QuerySuggestion[];
	diagnostics: Diagnostic[];
	applySuggestion: (suggestion: QuerySuggestion) => { raw: string; cursor: number };
}

export function useFhirQueryInput(
	options: UseFhirQueryInputOptions,
): UseFhirQueryInputReturn {
	const basePath = options.basePath ?? "/fhir";
	const [raw, setRawState] = useState(options.initialRaw ?? `${basePath}/`);
	const [cursorOffset, setCursorOffset] = useState(
		(options.initialRaw ?? `${basePath}/`).length,
	);

	const ast = useMemo(
		() => parseQueryAst(raw, basePath),
		[raw, basePath],
	);

	const context = useMemo(
		() =>
			getCursorContext(
				raw,
				cursorOffset,
				options.metadata.resourceTypes,
				basePath,
			),
		[raw, cursorOffset, options.metadata.resourceTypes, basePath],
	);

	const suggestions = useMemo(
		() => getSuggestions(context, options.metadata),
		[context, options.metadata],
	);

	const diagnostics = useMemo(
		() => computeDiagnostics(ast, options.metadata),
		[ast, options.metadata],
	);

	const setRaw = useCallback(
		(newRaw: string, newCursor: number) => {
			setRawState(newRaw);
			setCursorOffset(newCursor);
		},
		[],
	);

	const applySuggestion = useCallback(
		(suggestion: QuerySuggestion): { raw: string; cursor: number } => {
			const beforeCursor = raw.slice(0, cursorOffset);
			const afterCursor = raw.slice(cursorOffset);

			let newRaw: string;
			let newCursor: number;

			if (context.type === "root") {
				newRaw = suggestion.insertText;
				newCursor = suggestion.insertText.length;
			} else if (context.type === "api-endpoint") {
				newRaw = suggestion.insertText + afterCursor;
				newCursor = suggestion.insertText.length;
			} else if (
				context.type === "next-after-resource" ||
				context.type === "next-after-id"
			) {
				newRaw = beforeCursor + suggestion.insertText + afterCursor;
				newCursor = (beforeCursor + suggestion.insertText).length;
			} else if (context.type === "resource-type") {
				const baseUrl = `${basePath}/`;
				const segments = beforeCursor
					.replace(new RegExp(`^${escapeRegExp(basePath)}\\/?`), "")
					.split("/");
				segments[segments.length - 1] = suggestion.insertText;
				newRaw = baseUrl + segments.join("/") + afterCursor;
				newCursor = (baseUrl + segments.join("/")).length;
			} else if (
				context.type === "query-param" ||
				context.type === "query-modifier"
			) {
				const queryStart = beforeCursor.indexOf("?");
				const beforeQuery = beforeCursor.slice(0, queryStart + 1);
				const queryPart = beforeCursor.slice(queryStart + 1);
				const lastAmpersand = queryPart.lastIndexOf("&");
				const tokenStart = lastAmpersand === -1 ? 0 : lastAmpersand + 1;

				const newQuery =
					queryPart.slice(0, tokenStart) + suggestion.insertText;
				newRaw = beforeQuery + newQuery + afterCursor;
				newCursor = (beforeQuery + newQuery).length;
			} else {
				// Default: replace current segment
				const segments = beforeCursor.split("/");
				segments[segments.length - 1] = suggestion.insertText;
				newRaw = segments.join("/") + afterCursor;
				newCursor = segments.join("/").length;
			}

			setRawState(newRaw);
			setCursorOffset(newCursor);
			return { raw: newRaw, cursor: newCursor };
		},
		[raw, cursorOffset, context, basePath],
	);

	return {
		raw,
		setRaw,
		cursorOffset,
		ast,
		context,
		suggestions,
		diagnostics,
		applySuggestion,
	};
}

function escapeRegExp(s: string): string {
	return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
