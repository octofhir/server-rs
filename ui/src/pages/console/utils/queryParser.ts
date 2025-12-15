import type { ConsoleSearchParamToken } from "../state/consoleStore";

/**
 * Parse a query string into key-value pairs.
 * Handles URL encoding/decoding.
 *
 * @example
 * parseQueryString("name=John&birthdate=ge2000-01-01")
 * // => { name: "John", birthdate: "ge2000-01-01" }
 */
export function parseQueryString(query: string): Record<string, string> {
	const params: Record<string, string> = {};

	if (!query || query.trim() === "") {
		return params;
	}

	// Remove leading ? if present
	const cleanQuery = query.startsWith("?") ? query.slice(1) : query;

	// Split by & and parse each param
	for (const part of cleanQuery.split("&")) {
		const trimmed = part.trim();
		if (!trimmed) continue;

		const eqIndex = trimmed.indexOf("=");
		if (eqIndex === -1) {
			// Parameter without value (e.g., "_summary")
			params[decodeURIComponent(trimmed)] = "";
		} else {
			const key = decodeURIComponent(trimmed.slice(0, eqIndex));
			const value = decodeURIComponent(trimmed.slice(eqIndex + 1));
			params[key] = value;
		}
	}

	return params;
}

/**
 * Serialize key-value pairs into a query string.
 * Handles URL encoding.
 *
 * @example
 * serializeQueryParams({ name: "John", birthdate: "ge2000-01-01" })
 * // => "name=John&birthdate=ge2000-01-01"
 */
export function serializeQueryParams(params: Record<string, string>): string {
	const parts: string[] = [];

	for (const [key, value] of Object.entries(params)) {
		if (value === "" || value === undefined) {
			// Parameter without value
			parts.push(encodeURIComponent(key));
		} else {
			parts.push(`${encodeURIComponent(key)}=${encodeURIComponent(value)}`);
		}
	}

	return parts.join("&");
}

/**
 * Format a search param token into query string format.
 * Handles modifiers (e.g., "birthdate:ge=2000-01-01").
 *
 * @example
 * formatSearchParamToken({ code: "birthdate", modifier: "ge", value: "2000-01-01" })
 * // => "birthdate:ge=2000-01-01"
 */
export function formatSearchParamToken(token: ConsoleSearchParamToken): string {
	const modifier = token.modifier ? `:${token.modifier}` : "";
	const value = token.value ? `=${token.value}` : "";
	return `${token.code}${modifier}${value}`;
}

/**
 * Merge search param tokens and additional query params into a single query string.
 * Search params are formatted with FHIR modifiers (e.g., "name:exact=John").
 * Query params are merged as-is.
 *
 * @example
 * mergeSearchParamsAndQuery(
 *   [{ code: "name", modifier: "exact", value: "John" }],
 *   { _count: "10" }
 * )
 * // => "name:exact=John&_count=10"
 */
export function mergeSearchParamsAndQuery(
	searchParams: ConsoleSearchParamToken[],
	queryParams: Record<string, string>,
): string {
	const parts: string[] = [];

	// Add formatted search params
	for (const token of searchParams) {
		const formatted = formatSearchParamToken(token);
		if (formatted) {
			parts.push(formatted);
		}
	}

	// Add additional query params
	for (const [key, value] of Object.entries(queryParams)) {
		if (value === "" || value === undefined) {
			parts.push(encodeURIComponent(key));
		} else {
			parts.push(`${encodeURIComponent(key)}=${encodeURIComponent(value)}`);
		}
	}

	return parts.join("&");
}

/**
 * Extract modifier from a parameter key.
 * FHIR search parameters can have modifiers (e.g., "name:exact" -> modifier is "exact").
 *
 * @example
 * extractModifier("name:exact")
 * // => { code: "name", modifier: "exact" }
 *
 * extractModifier("name")
 * // => { code: "name", modifier: undefined }
 */
export function extractModifier(paramKey: string): {
	code: string;
	modifier?: string;
} {
	const colonIndex = paramKey.indexOf(":");
	if (colonIndex === -1) {
		return { code: paramKey };
	}

	return {
		code: paramKey.slice(0, colonIndex),
		modifier: paramKey.slice(colonIndex + 1),
	};
}
