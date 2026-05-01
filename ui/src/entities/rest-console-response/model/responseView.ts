import type { FhirBundle, FhirResource, HttpMethod } from "@/shared/api";

export interface RequestResponse {
	id: string;
	status: number;
	statusText: string;
	durationMs: number;
	body?: unknown;
	headers?: Record<string, string>;
	requestedAt: string;
	requestMethod: HttpMethod;
	requestPath: string;
	requestBody?: string;
	requestHeaders?: Record<string, string>;
}

export interface FhirOperationOutcome {
	resourceType: "OperationOutcome";
	issue?: Array<{
		severity?: "fatal" | "error" | "warning" | "information";
		code?: string;
		diagnostics?: string;
		location?: string[];
	}>;
}

export interface FhirBundleResourceEntry {
	resource: FhirResource;
	fullUrl?: string;
}

export type ConsoleResponseStatusTone = "success" | "danger" | "warning";

export function getConsoleResponseStatusTone(status: number): ConsoleResponseStatusTone {
	if (status >= 200 && status < 300) return "success";
	if (status >= 400) return "danger";
	return "warning";
}

export function isConsoleResponseSuccess(status: number): boolean {
	return status >= 200 && status < 300;
}

export function isConsoleResponseError(status: number): boolean {
	return status >= 400;
}

export function isFhirOperationOutcome(body: unknown): body is FhirOperationOutcome {
	return (
		typeof body === "object" &&
		body !== null &&
		"resourceType" in body &&
		body.resourceType === "OperationOutcome"
	);
}

export function isFhirBundle(body: unknown): body is FhirBundle {
	return (
		typeof body === "object" &&
		body !== null &&
		"resourceType" in body &&
		body.resourceType === "Bundle"
	);
}

export function getOperationOutcomeSummary(
	body: unknown,
): { message: string; severity?: string } | null {
	if (!isFhirOperationOutcome(body)) return null;

	const firstIssue = body.issue?.[0];
	return {
		message: firstIssue?.diagnostics || "An error occurred",
		severity: firstIssue?.severity,
	};
}

export function getBundleResourceEntries(body: unknown): FhirBundleResourceEntry[] {
	if (!isFhirBundle(body)) return [];

	return (body.entry ?? []).filter(
		(entry): entry is FhirBundleResourceEntry =>
			Boolean(entry.resource?.resourceType),
	);
}

export function getResponseDefaultTab(response: RequestResponse): "results" | "body" {
	return getBundleResourceEntries(response.body).length > 0 ? "results" : "body";
}
