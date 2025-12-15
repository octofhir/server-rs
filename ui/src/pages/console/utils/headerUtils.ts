/**
 * Default FHIR headers used in REST console requests.
 */
export const DEFAULT_FHIR_HEADERS = {
	Accept: "application/fhir+json",
	"Content-Type": "application/fhir+json",
} as const;

/**
 * Common FHIR-specific HTTP headers for autocomplete suggestions.
 */
export const COMMON_FHIR_HEADERS = [
	"Prefer",
	"If-Match",
	"If-None-Match",
	"If-Modified-Since",
	"If-None-Exist",
	"X-Request-Id",
	"Authorization",
	"Cache-Control",
] as const;

/**
 * Validate HTTP headers and return validation errors.
 * Checks for:
 * - Duplicate keys (case-insensitive)
 * - Invalid header names (basic validation)
 *
 * @returns Array of error messages (empty if valid)
 */
export function validateHeaders(headers: Record<string, string>): string[] {
	const errors: string[] = [];
	const seen = new Map<string, string>();

	for (const [key, value] of Object.entries(headers)) {
		const lowerKey = key.toLowerCase();

		// Check for duplicates (case-insensitive)
		if (seen.has(lowerKey)) {
			errors.push(`Duplicate header: "${key}" (conflicts with "${seen.get(lowerKey)}")`);
		} else {
			seen.set(lowerKey, key);
		}

		// Basic header name validation (alphanumeric + hyphens)
		if (!/^[a-zA-Z0-9-]+$/.test(key)) {
			errors.push(`Invalid header name: "${key}" (use only alphanumeric and hyphens)`);
		}

		// Check for empty header names
		if (key.trim() === "") {
			errors.push("Header name cannot be empty");
		}

		// Warn about empty values (not necessarily an error)
		if (value.trim() === "") {
			// Note: Empty values are valid in HTTP but might be unintentional
			// We don't add this as an error, just log for debugging
		}
	}

	return errors;
}

/**
 * Merge default headers with custom headers.
 * Custom headers override defaults with the same key (case-insensitive).
 *
 * @example
 * mergeHeaders(
 *   { Accept: "application/fhir+json" },
 *   { accept: "application/json" }
 * )
 * // => { Accept: "application/json" } (custom override)
 */
export function mergeHeaders(
	defaults: Record<string, string>,
	custom: Record<string, string>,
): Record<string, string> {
	const merged: Record<string, string> = { ...defaults };

	// Override defaults with custom headers (case-insensitive matching)
	for (const [customKey, customValue] of Object.entries(custom)) {
		const lowerCustomKey = customKey.toLowerCase();

		// Find matching default key (case-insensitive)
		let matched = false;
		for (const defaultKey of Object.keys(defaults)) {
			if (defaultKey.toLowerCase() === lowerCustomKey) {
				// Override with custom value but keep default casing
				merged[defaultKey] = customValue;
				matched = true;
				break;
			}
		}

		// If no match, add new custom header
		if (!matched) {
			merged[customKey] = customValue;
		}
	}

	return merged;
}

/**
 * Check if a header key is a default FHIR header (case-insensitive).
 */
export function isDefaultHeader(key: string): boolean {
	const lowerKey = key.toLowerCase();
	return Object.keys(DEFAULT_FHIR_HEADERS).some(
		(defaultKey) => defaultKey.toLowerCase() === lowerKey,
	);
}

/**
 * Get suggested header names based on partial input.
 * Returns common FHIR headers that match the query.
 *
 * @example
 * getSuggestedHeaders("if")
 * // => ["If-Match", "If-None-Match", "If-Modified-Since", "If-None-Exist"]
 */
export function getSuggestedHeaders(query: string): string[] {
	const lowerQuery = query.toLowerCase();
	return COMMON_FHIR_HEADERS.filter((header) =>
		header.toLowerCase().includes(lowerQuery),
	);
}
