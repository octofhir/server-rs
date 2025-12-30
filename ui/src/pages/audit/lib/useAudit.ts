import { useQuery, useInfiniteQuery } from "@tanstack/react-query";
import { fhirClient } from "@/shared/api/fhirClient";
import type {
	AuditEvent,
	AuditEventUIFilters,
	AuditAnalytics,
	AuditAction,
	AuditOutcome,
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

// Helper to build FHIR search params from UI filters
function buildFhirSearchParams(filters: AuditEventUIFilters): Record<string, string | number> {
	const params: Record<string, string | number> = {};

	// Text search
	if (filters.search) {
		params._content = filters.search;
	}

	// Action filter - map UI actions to FHIR subtype
	if (filters.action?.length) {
		params.subtype = filters.action.join(",");
	}

	// Outcome filter - map to FHIR outcome codes
	if (filters.outcome?.length) {
		const outcomeMap: Record<string, string> = {
			success: "0",
			partial: "4",
			failure: "8",
		};
		params.outcome = filters.outcome.map((o) => outcomeMap[o] || o).join(",");
	}

	// Actor type filter
	if (filters.actorType?.length) {
		params["agent-type"] = filters.actorType.join(",");
	}

	// Actor ID filter
	if (filters.actorId) {
		params.agent = filters.actorId;
	}

	// Resource type filter
	if (filters.resourceType) {
		params["entity-type"] = filters.resourceType;
	}

	// Resource ID filter
	if (filters.resourceId) {
		params.entity = filters.resourceId;
	}

	// Time range filter
	if (filters.startTime) {
		params.date = `ge${filters.startTime}`;
	}
	if (filters.endTime) {
		// If we already have a date param, we need to add another
		if (params.date) {
			params["date:le"] = filters.endTime;
		} else {
			params.date = `le${filters.endTime}`;
		}
	}

	// IP address filter
	if (filters.ipAddress) {
		params.address = filters.ipAddress;
	}

	// Default pagination
	params._count = 50;
	params._sort = "-date";

	return params;
}

// Transform a raw FHIR AuditEvent resource to our UI AuditEvent type
function transformFhirAuditEvent(resource: FhirResource): AuditEvent {
	// Extract action from subtype
	const subtypeCode = resource.subtype?.[0]?.code || "unknown";
	const action = subtypeCode as AuditAction;

	// Map FHIR outcome code to UI outcome
	const outcomeCode = resource.outcome as string;
	let outcome: AuditOutcome = "success";
	if (outcomeCode === "4") outcome = "partial";
	else if (outcomeCode === "8" || outcomeCode === "12") outcome = "failure";

	// Extract actor from first agent
	const agent = resource.agent?.[0];
	const agentTypeCode = agent?.type?.coding?.[0]?.code;
	let actorType: "user" | "client" | "system" = "system";
	if (agentTypeCode === "110153") actorType = "user"; // Source Role ID = user
	else if (agentTypeCode === "110150") actorType = "client"; // Application

	const actor = {
		type: actorType,
		id: agent?.who?.identifier?.value || agent?.who?.reference,
		name: agent?.who?.display,
		reference: agent?.who?.reference,
	};

	// Extract source
	const source = {
		observer: resource.source?.observer?.display,
		site: resource.source?.site,
		ipAddress: resource.extension?.find(
			(e: { url: string }) => e.url === "http://octofhir.io/StructureDefinition/audit-source-ip"
		)?.valueString,
		userAgent: resource.extension?.find(
			(e: { url: string }) => e.url === "http://octofhir.io/StructureDefinition/audit-user-agent"
		)?.valueString,
	};

	// Extract target from first entity
	const entity = resource.entity?.[0];
	const target = entity
		? {
				reference: entity.what?.reference,
				resourceType: entity.what?.type || entity.what?.reference?.split("/")[0],
				resourceId: entity.what?.reference?.split("/")[1],
				query: entity.description,
			}
		: undefined;

	// Extract context from extensions
	const context = {
		requestId: resource.extension?.find(
			(e: { url: string }) => e.url === "http://octofhir.io/StructureDefinition/audit-request-id"
		)?.valueString,
		sessionId: resource.extension?.find(
			(e: { url: string }) => e.url === "http://octofhir.io/StructureDefinition/audit-session-id"
		)?.valueString,
		clientId: resource.extension?.find(
			(e: { url: string }) => e.url === "http://octofhir.io/StructureDefinition/audit-client-id"
		)?.valueString,
	};

	return {
		resourceType: "AuditEvent",
		id: resource.id || "",
		timestamp: resource.recorded || new Date().toISOString(),
		action,
		actionCode: resource.action,
		outcome,
		outcomeCode: outcomeCode as AuditEvent["outcomeCode"],
		outcomeDescription: resource.outcomeDesc,
		actor,
		source,
		target,
		context,
	};
}

// Transform FHIR Bundle to our list response
function transformBundleToList(bundle: FhirBundle): {
	events: AuditEvent[];
	total: number;
	hasMore: boolean;
	nextCursor?: string;
} {
	const events = (bundle.entry || []).map((entry) => {
		return transformFhirAuditEvent(entry.resource);
	});

	const nextLink = bundle.link?.find((l) => l.relation === "next");

	return {
		events,
		total: bundle.total || events.length,
		hasMore: !!nextLink,
		nextCursor: nextLink?.url,
	};
}

// API functions using FHIR client
async function fetchAuditEvents(
	filters: AuditEventUIFilters,
	cursor?: string
): Promise<ReturnType<typeof transformBundleToList>> {
	// If we have a cursor (next page URL), use it directly
	if (cursor) {
		const response = await fhirClient.customRequest<FhirBundle>({
			method: "GET",
			url: cursor,
		});
		return transformBundleToList(response.data);
	}

	// Build search params from filters
	const params = buildFhirSearchParams(filters);
	const bundle = await fhirClient.search("AuditEvent", params);
	return transformBundleToList(bundle);
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
	const params = buildFhirSearchParams(filters);
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
