import { useQuery, useInfiniteQuery } from "@tanstack/react-query";
import {
	buildAuditFhirSearchParams,
	transformAuditBundleToList,
	transformFhirAuditEvent,
} from "@/entities/audit-event";
import { fhirClient } from "@/shared/api/fhirClient";
import type {
	AuditEvent,
	AuditEventUIFilters,
	AuditAnalytics,
	FhirBundle,
	FhirResource,
} from "@/shared/api/types";

// Query keys
export const auditKeys = {
	all: ["audit"] as const,
	lists: () => [...auditKeys.all, "list"] as const,
	list: (filters: AuditEventUIFilters) => [...auditKeys.lists(), filters] as const,
	details: () => [...auditKeys.all, "detail"] as const,
	detail: (id: string) => [...auditKeys.details(), id] as const,
	analytics: (timeRange?: { start: string; end: string }) =>
		[...auditKeys.all, "analytics", timeRange] as const,
};

// API functions using FHIR client
async function fetchAuditEvents(
	filters: AuditEventUIFilters,
	cursor?: string
): Promise<ReturnType<typeof transformAuditBundleToList>> {
	// If we have a cursor (next page URL), use it directly
	if (cursor) {
		const response = await fhirClient.customRequest<FhirBundle>({
			method: "GET",
			url: cursor,
		});
		return transformAuditBundleToList(response.data);
	}

	// Build search params from filters
	const params = buildAuditFhirSearchParams(filters);
	const bundle = await fhirClient.search("AuditEvent", params);
	return transformAuditBundleToList(bundle);
}

async function fetchAuditEvent(id: string): Promise<AuditEvent> {
	const resource = await fhirClient.read<FhirResource>("AuditEvent", id);
	return transformFhirAuditEvent(resource);
}

// Analytics uses custom admin endpoint (not standard FHIR)
async function fetchAuditAnalytics(timeRange?: {
	start: string;
	end: string;
}): Promise<AuditAnalytics> {
	const params = new URLSearchParams();
	if (timeRange?.start) params.append("start", timeRange.start);
	if (timeRange?.end) params.append("end", timeRange.end);

	const response = await fetch(`/admin/audit/$analytics?${params.toString()}`, {
		credentials: "include",
		headers: {
			Accept: "application/json",
		},
	});

	if (!response.ok) {
		const error = await response.json().catch(() => ({ message: response.statusText }));
		throw new Error(error.message || `HTTP ${response.status}`);
	}

	return response.json();
}

// Hooks
export function useAuditEvents(filters: AuditEventUIFilters = {}) {
	return useInfiniteQuery({
		queryKey: auditKeys.list(filters),
		queryFn: ({ pageParam }) => fetchAuditEvents(filters, pageParam as string | undefined),
		initialPageParam: undefined as string | undefined,
		getNextPageParam: (lastPage) => (lastPage.hasMore ? lastPage.nextCursor : undefined),
	});
}

export function useAuditEvent(id: string | null) {
	return useQuery({
		queryKey: auditKeys.detail(id || ""),
		queryFn: () => {
			if (!id) throw new Error("ID is required");
			return fetchAuditEvent(id);
		},
		enabled: !!id,
	});
}

export function useAuditAnalytics(timeRange?: { start: string; end: string }) {
	return useQuery({
		queryKey: auditKeys.analytics(timeRange),
		queryFn: () => fetchAuditAnalytics(timeRange),
	});
}

// Export function for audit logs - uses FHIR $export or custom operation
export async function exportAuditLogs(
	filters: AuditEventUIFilters,
	format: "json" | "csv"
): Promise<void> {
	const params = buildAuditFhirSearchParams(filters);
	const searchParams = new URLSearchParams();

	Object.entries(params).forEach(([key, value]) => {
		searchParams.append(key, String(value));
	});
	searchParams.append("_format", format === "json" ? "application/fhir+json" : "text/csv");

	// Use custom export operation
	const response = await fetch(`/fhir/AuditEvent/$export?${searchParams.toString()}`, {
		credentials: "include",
	});

	if (!response.ok) {
		const error = await response.json().catch(() => ({ message: response.statusText }));
		throw new Error(error.message || `HTTP ${response.status}`);
	}

	const blob = await response.blob();
	const url = window.URL.createObjectURL(blob);
	const a = document.createElement("a");
	a.href = url;
	a.download = `audit-logs-${new Date().toISOString().split("T")[0]}.${format}`;
	document.body.appendChild(a);
	a.click();
	window.URL.revokeObjectURL(url);
	document.body.removeChild(a);
}

// Re-export the filter type for convenience
export type { AuditEventUIFilters as AuditEventFilters } from "@/shared/api/types";
