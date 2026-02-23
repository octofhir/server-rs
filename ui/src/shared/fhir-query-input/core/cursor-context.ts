import type { CursorContext, CursorContextType } from "./types";

const DEFAULT_BASE_PATH = "/fhir";

export function getCursorContext(
	raw: string,
	cursorOffset: number,
	resourceTypes: string[],
	basePath: string = DEFAULT_BASE_PATH,
): CursorContext {
	const beforeCursor = raw.slice(0, cursorOffset);

	// Check if we're in /api path
	if (beforeCursor.startsWith("/api")) {
		const apiPath = beforeCursor.replace(/^\/api\/?/, "");
		return makeContext("api-endpoint", apiPath, beforeCursor);
	}

	// Check root
	if (beforeCursor === "/" || beforeCursor === "") {
		return makeContext("root", beforeCursor, beforeCursor);
	}

	// Remove basePath prefix
	const basePathWithSlash = `${basePath}/`;
	let relativePath: string;
	if (beforeCursor.startsWith(basePathWithSlash)) {
		relativePath = beforeCursor.slice(basePathWithSlash.length);
	} else if (beforeCursor.startsWith(basePath)) {
		relativePath = beforeCursor.slice(basePath.length);
		if (relativePath.startsWith("/")) {
			relativePath = relativePath.slice(1);
		}
	} else {
		relativePath = beforeCursor.replace(/^\/fhir\/?/, "");
	}

	// Check if we're in query params
	const queryStart = relativePath.indexOf("?");
	if (queryStart !== -1) {
		return parseQueryCursorContext(relativePath, queryStart, raw, cursorOffset);
	}

	// Parse path segments
	const segments = relativePath.split("/").filter(Boolean);
	const currentSegment = segments[segments.length - 1] || "";
	const hasTrailingSlash =
		beforeCursor.endsWith("/") && !beforeCursor.endsWith(`${basePath}/`);
	const afterCursor = raw.slice(cursorOffset);

	if (segments.length === 0) {
		return makeContext("resource-type", currentSegment, beforeCursor);
	}

	if (segments.length === 1) {
		// Trailing slash after resource type -> suggest ID
		if (hasTrailingSlash && resourceTypes.includes(segments[0])) {
			return makeContext("resource-id", "", beforeCursor);
		}

		// Complete resource type -> suggest next steps
		const isComplete =
			resourceTypes.includes(segments[0]) &&
			beforeCursor.endsWith(segments[0]) &&
			!currentSegment.startsWith("$");

		if (isComplete) {
			return {
				type: "next-after-resource",
				resourceType: segments[0],
				fragment: "",
				span: { start: cursorOffset, end: cursorOffset },
			};
		}

		// $operation — system-level if the segment itself is the $ part (no resource type before)
		if (currentSegment.startsWith("$")) {
			return {
				type: "system-operation",
				fragment: currentSegment,
				span: {
					start: cursorOffset - currentSegment.length,
					end: cursorOffset,
				},
			};
		}

		const char = afterCursor[0];
		if (char === "$") {
			return {
				type: "type-operation",
				resourceType: segments[0],
				fragment: currentSegment,
				span: {
					start: cursorOffset - currentSegment.length,
					end: cursorOffset,
				},
			};
		}

		return makeContext("resource-type", currentSegment, beforeCursor);
	}

	if (segments.length === 2) {
		// Second segment is $operation -> type-level operation
		if (segments[1].startsWith("$")) {
			return {
				type: "type-operation",
				resourceType: segments[0],
				fragment: segments[1],
				span: {
					start: cursorOffset - segments[1].length,
					end: cursorOffset,
				},
			};
		}

		// After resource ID -> suggest instance operations
		if (beforeCursor.endsWith(segments[1])) {
			return {
				type: "next-after-id",
				resourceType: segments[0],
				resourceId: segments[1],
				fragment: "",
				span: { start: cursorOffset, end: cursorOffset },
			};
		}

		// Trailing slash after ID -> suggest instance operations
		if (hasTrailingSlash) {
			return {
				type: "instance-operation",
				resourceType: segments[0],
				resourceId: segments[1],
				fragment: "",
				span: { start: cursorOffset, end: cursorOffset },
			};
		}

		return makeContext("resource-id", currentSegment, beforeCursor);
	}

	if (segments.length === 3) {
		// ResourceType/id/$operation
		if (segments[2].startsWith("$")) {
			return {
				type: "instance-operation",
				resourceType: segments[0],
				resourceId: segments[1],
				fragment: segments[2],
				span: {
					start: cursorOffset - segments[2].length,
					end: cursorOffset,
				},
			};
		}
	}

	return makeContext("unknown", currentSegment, beforeCursor);
}

function parseQueryCursorContext(
	relativePath: string,
	queryStart: number,
	raw: string,
	cursorOffset: number,
): CursorContext {
	const queryPart = relativePath.slice(queryStart + 1);
	const tokens = queryPart.split("&");
	const lastToken = tokens[tokens.length - 1] || "";

	// Extract resource type from path part
	const pathOnly = relativePath.slice(0, queryStart);
	const pathSegments = pathOnly.split("/").filter(Boolean);
	const resourceType = pathSegments[0];

	const colonIndex = lastToken.indexOf(":");
	const equalsIndex = lastToken.indexOf("=");

	// Typing modifier: name:mod (colon present, no equals yet)
	if (colonIndex !== -1 && equalsIndex === -1) {
		const paramName = lastToken.slice(0, colonIndex);
		const modifierPart = lastToken.slice(colonIndex + 1);
		return {
			type: "query-modifier",
			resourceType,
			paramName,
			fragment: modifierPart,
			span: {
				start: cursorOffset - modifierPart.length,
				end: cursorOffset,
			},
		};
	}

	// Typing param name (no equals)
	if (equalsIndex === -1) {
		return {
			type: "query-param",
			resourceType,
			fragment: lastToken,
			span: {
				start: cursorOffset - lastToken.length,
				end: cursorOffset,
			},
		};
	}

	// Typing value (after equals)
	const paramPart = lastToken.slice(0, equalsIndex);
	const valuePart = lastToken.slice(equalsIndex + 1);
	const colonInParam = paramPart.indexOf(":");
	const paramName =
		colonInParam === -1 ? paramPart : paramPart.slice(0, colonInParam);

	return {
		type: "query-value",
		resourceType,
		paramName,
		fragment: valuePart,
		span: {
			start: cursorOffset - valuePart.length,
			end: cursorOffset,
		},
	};
}

function makeContext(
	type: CursorContextType,
	fragment: string,
	beforeCursor: string,
): CursorContext {
	return {
		type,
		fragment,
		span: {
			start: beforeCursor.length - fragment.length,
			end: beforeCursor.length,
		},
	};
}
