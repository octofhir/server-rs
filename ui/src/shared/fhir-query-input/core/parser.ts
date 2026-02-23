import type {
	PathNode,
	QueryAst,
	QueryParamNode,
	QueryValueNode,
	TextSpan,
} from "./types";

const SEARCH_PREFIXES = new Set([
	"eq",
	"ne",
	"gt",
	"lt",
	"ge",
	"le",
	"sa",
	"eb",
	"ap",
]);

const DEFAULT_BASE_PATH = "/fhir";

export function parseQueryAst(
	raw: string,
	basePath: string = DEFAULT_BASE_PATH,
): QueryAst {
	const questionIdx = raw.indexOf("?");
	const pathPart = questionIdx === -1 ? raw : raw.slice(0, questionIdx);
	const queryPart = questionIdx === -1 ? "" : raw.slice(questionIdx + 1);

	const path = parsePath(pathPart, basePath);
	const params =
		queryPart.length > 0 ? parseQueryParams(queryPart, questionIdx + 1) : [];

	return { raw, basePath, path, params };
}

function parsePath(pathPart: string, basePath: string): PathNode {
	// Handle /api paths
	if (pathPart.startsWith("/api")) {
		return {
			kind: "api-endpoint",
			path: pathPart,
			span: { start: 0, end: pathPart.length },
		};
	}

	// Handle root or empty
	if (pathPart === "/" || pathPart === "") {
		return {
			kind: "root",
			span: { start: 0, end: pathPart.length },
		};
	}

	// Strip basePath
	let relative = pathPart;
	if (pathPart.startsWith(basePath)) {
		relative = pathPart.slice(basePath.length);
	}
	if (relative.startsWith("/")) {
		relative = relative.slice(1);
	}

	// Remove trailing slash for parsing
	const trimmedRelative = relative.endsWith("/")
		? relative.slice(0, -1)
		: relative;
	const segments = trimmedRelative.split("/").filter(Boolean);
	const pathSpan: TextSpan = { start: 0, end: pathPart.length };

	if (segments.length === 0) {
		return { kind: "root", span: pathSpan };
	}

	// Check for system operation: $operation
	if (segments.length === 1 && segments[0].startsWith("$")) {
		return {
			kind: "system-operation",
			operation: segments[0],
			span: pathSpan,
		};
	}

	// Single segment: resource type
	if (segments.length === 1) {
		return {
			kind: "resource-type",
			resourceType: segments[0],
			span: pathSpan,
		};
	}

	// Two segments
	if (segments.length === 2) {
		// ResourceType/$operation
		if (segments[1].startsWith("$")) {
			return {
				kind: "type-operation",
				resourceType: segments[0],
				operation: segments[1],
				span: pathSpan,
			};
		}
		// ResourceType/id
		return {
			kind: "resource-instance",
			resourceType: segments[0],
			id: segments[1],
			span: pathSpan,
		};
	}

	// Three segments: ResourceType/id/$operation
	if (segments.length === 3 && segments[2].startsWith("$")) {
		return {
			kind: "instance-operation",
			resourceType: segments[0],
			id: segments[1],
			operation: segments[2],
			span: pathSpan,
		};
	}

	return {
		kind: "unknown",
		text: pathPart,
		span: pathSpan,
	};
}

function parseQueryParams(
	queryPart: string,
	globalOffset: number,
): QueryParamNode[] {
	if (!queryPart) return [];

	const tokens = queryPart.split("&");
	const params: QueryParamNode[] = [];
	let pos = 0;

	for (const token of tokens) {
		const tokenStart = globalOffset + pos;
		const tokenEnd = tokenStart + token.length;

		const eqIdx = token.indexOf("=");
		const key = eqIdx === -1 ? token : token.slice(0, eqIdx);
		const rawValue = eqIdx === -1 ? "" : token.slice(eqIdx + 1);

		const colonIdx = key.indexOf(":");
		const name = colonIdx === -1 ? key : key.slice(0, colonIdx);
		const modifier = colonIdx === -1 ? undefined : key.slice(colonIdx + 1);

		const values = parseValues(
			rawValue,
			tokenStart + (eqIdx === -1 ? token.length : eqIdx + 1),
		);

		params.push({
			name,
			modifier,
			values,
			isSpecial: name.startsWith("_"),
			span: { start: tokenStart, end: tokenEnd },
		});

		pos += token.length + 1; // +1 for &
	}

	return params;
}

function parseValues(rawValue: string, globalOffset: number): QueryValueNode[] {
	if (!rawValue) return [];

	const parts = rawValue.split(",");
	const values: QueryValueNode[] = [];
	let pos = 0;

	for (const part of parts) {
		const start = globalOffset + pos;
		const end = start + part.length;

		let prefix: string | undefined;
		if (part.length >= 2) {
			const candidate = part.slice(0, 2);
			if (SEARCH_PREFIXES.has(candidate)) {
				prefix = candidate;
			}
		}

		values.push({
			raw: part,
			prefix,
			span: { start, end },
		});

		pos += part.length + 1; // +1 for ,
	}

	return values;
}
