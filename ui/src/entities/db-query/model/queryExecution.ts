import { ApiResponseError } from "@/shared/api/serverApi";

const QUERY_TIMEOUT_MESSAGE =
	"Request timeout. Query may still be running. Check Active queries.";

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null;
}

function getString(value: unknown): string | undefined {
	return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

export function formatOperationOutcomeDetails(payload: Record<string, unknown>): string | null {
	if (payload.resourceType !== "OperationOutcome" || !Array.isArray(payload.issue)) {
		return null;
	}

	const lines = payload.issue
		.map((rawIssue) => {
			if (!isRecord(rawIssue)) return null;
			const severity = getString(rawIssue.severity);
			const code = getString(rawIssue.code);
			const diagnostics = getString(rawIssue.diagnostics);
			const detailsText = isRecord(rawIssue.details)
				? getString(rawIssue.details.text)
				: undefined;
			const location = Array.isArray(rawIssue.location)
				? rawIssue.location.filter(
						(value): value is string => typeof value === "string" && value.trim().length > 0,
					)
				: [];
			const expression = Array.isArray(rawIssue.expression)
				? rawIssue.expression.filter(
						(value): value is string => typeof value === "string" && value.trim().length > 0,
					)
				: [];

			const parts: string[] = [];
			if (severity || code) {
				parts.push(`[${severity ?? "unknown"}${code ? `/${code}` : ""}]`);
			}
			if (diagnostics || detailsText) {
				parts.push(diagnostics ?? detailsText ?? "");
			}
			if (expression.length > 0) {
				parts.push(`expr: ${expression.join(", ")}`);
			} else if (location.length > 0) {
				parts.push(`loc: ${location.join(", ")}`);
			}

			return parts.length > 0 ? parts.join(" ") : null;
		})
		.filter((line): line is string => Boolean(line));

	return lines.length > 0 ? lines.join("\n") : null;
}

export function formatApiErrorPayload(payload: unknown): string | null {
	if (typeof payload === "string" && payload.trim()) {
		return payload.trim();
	}
	if (!isRecord(payload)) {
		return null;
	}

	const operationOutcomeDetails = formatOperationOutcomeDetails(payload);
	if (operationOutcomeDetails) {
		return operationOutcomeDetails;
	}

	const fallbackMessage =
		getString(payload.message) ??
		getString(payload.error) ??
		getString(payload.diagnostics);

	if (fallbackMessage) {
		return fallbackMessage;
	}

	try {
		return JSON.stringify(payload, null, 2);
	} catch {
		return null;
	}
}

export function formatSqlError(error: unknown): string {
	if (error instanceof Error && error.message === "Request timeout") {
		return QUERY_TIMEOUT_MESSAGE;
	}

	if (error instanceof ApiResponseError) {
		const details = formatApiErrorPayload(error.responseData);
		return details ? `${error.message}\n${details}` : error.message;
	}

	if (error instanceof Error) {
		return error.message;
	}

	return "Unknown error";
}

export function isSelectLikeQuery(query: string): boolean {
	const trimmed = query.trimStart().toUpperCase();
	return trimmed.startsWith("SELECT") || trimmed.startsWith("WITH");
}

export function applyResultLimit(query: string, limitValue: string): string {
	if (limitValue === "none") {
		return query;
	}
	if (!isSelectLikeQuery(query) || /\bLIMIT\b/i.test(query)) {
		return query;
	}

	const limit = Number.parseInt(limitValue, 10);
	if (!Number.isFinite(limit) || limit <= 0) {
		return query;
	}

	const trimmed = query.trimEnd();
	if (!trimmed) {
		return query;
	}

	const hasSemicolon = trimmed.endsWith(";");
	const baseQuery = hasSemicolon ? trimmed.slice(0, -1) : trimmed;
	return `${baseQuery}\nLIMIT ${limit}${hasSemicolon ? ";" : ""}`;
}

export function parseTimeoutMs(timeoutValue: string): number | undefined {
	const parsed = Number.parseInt(timeoutValue, 10);
	if (!Number.isFinite(parsed) || parsed <= 0) {
		return undefined;
	}
	return parsed;
}

