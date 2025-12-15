import type { HttpMethod } from "@/shared/api";
import type { ConsoleSearchParamToken } from "@/pages/console/state/searchParams";

export interface PathTokenBuildInput {
	resourceType?: string;
	resourceId?: string;
	interaction?: string | null;
	operation?: string;
	searchParams: ConsoleSearchParamToken[];
}

export interface TokenizeResult extends PathTokenBuildInput {
	method?: HttpMethod;
}

const BASE_PATH = "/fhir";

export function buildPathTokens(input: PathTokenBuildInput, basePath = BASE_PATH): string[] {
	const tokens: string[] = [basePath];
	if (input.resourceType) {
		tokens.push(input.resourceType);
	}
	if (input.resourceId) {
		tokens.push(input.resourceId);
	}
	if (input.interaction) {
		tokens.push(input.interaction);
	}
	if (input.operation) {
		tokens.push(input.operation.startsWith("$") ? input.operation : `$${input.operation}`);
	}
	const query = buildQueryString(input.searchParams);
	if (query) {
		tokens.push(`?${query}`);
	}
	return tokens;
}

export function buildPathPreview(input: PathTokenBuildInput, basePath = BASE_PATH): string {
	const tokens = buildPathTokens(input, basePath);
	return tokens
		.map((token, index) => {
			if (index === 0) {
				return token;
			}
			if (token.startsWith("?") || token.startsWith("&")) {
				return token;
			}
			if (token.startsWith("$") || token.startsWith("_")) {
				return `/${token}`;
			}
			return `/${token}`;
		})
		.join("");
}

export function buildQueryString(params: ConsoleSearchParamToken[]): string {
	return params
		.map((param) => {
			const modifier = param.modifier ? `:${param.modifier}` : "";
			const value = param.value ?? "";
			return `${encodeURIComponent(param.code + modifier)}=${encodeURIComponent(value)}`;
		})
		.filter((part) => part.trim().length > 0)
		.join("&");
}

export function tokenizePathInput(
	input: string,
	basePath = BASE_PATH,
): TokenizeResult {
	const result: TokenizeResult = { searchParams: [] };
	if (!input.trim()) {
		return result;
	}

	const methodMatch = input.match(/^(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s+/i);
	let working = input.trim();
	if (methodMatch) {
		result.method = methodMatch[1].toUpperCase() as HttpMethod;
		working = working.slice(methodMatch[0].length).trim();
	}

	if (!working.startsWith(basePath)) {
		return result;
	}

	working = working.slice(basePath.length);
	if (working.startsWith("/")) {
		working = working.slice(1);
	}

	let pathPart = working;
	let queryString = "";
	const queryIndex = working.indexOf("?");
	if (queryIndex >= 0) {
		pathPart = working.slice(0, queryIndex);
		queryString = working.slice(queryIndex + 1);
	}

	const segments = pathPart.split("/").filter((segment) => segment.length > 0);
	if (segments[0]) {
		result.resourceType = segments[0];
	}
	if (segments[1] && !segments[1].startsWith("_") && !segments[1].startsWith("$")) {
		result.resourceId = segments[1];
	}
	for (let i = 1; i < segments.length; i += 1) {
		const segment = segments[i];
		if (segment.startsWith("_")) {
			result.interaction = segment;
		} else if (segment.startsWith("$")) {
			result.operation = segment;
		}
	}

	if (queryString) {
		const tokens = queryString.split("&");
		for (const rawToken of tokens) {
			const [rawKey, ...rest] = rawToken.split("=");
			const key = decodeURIComponent(rawKey ?? "").trim();
			const value = decodeURIComponent(rest.join("=") ?? "").trim();
			if (!key) continue;
			const [code, modifier] = key.split(":");
			result.searchParams.push({
				id: cryptoRandomId(),
				code,
				modifier,
				value,
				fromMetadata: false,
			});
		}
	}

	return result;
}

export function cryptoRandomId() {
	if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
		return crypto.randomUUID();
	}
	return Math.random().toString(36).slice(2, 10);
}
