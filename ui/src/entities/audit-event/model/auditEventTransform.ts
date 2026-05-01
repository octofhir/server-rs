import type {
	AuditAction,
	AuditEvent,
	AuditEventListResponse,
	AuditEventUIFilters,
	AuditOutcome,
	FhirBundle,
	FhirResource,
} from "@/shared/api/types";

export function buildAuditFhirSearchParams(
	filters: AuditEventUIFilters,
): Record<string, string | number> {
	const params: Record<string, string | number> = {};

	if (filters.search) {
		params._content = filters.search;
	}

	if (filters.action?.length) {
		params.subtype = filters.action.join(",");
	}

	if (filters.outcome?.length) {
		const outcomeMap: Record<string, string> = {
			success: "0",
			partial: "4",
			failure: "8",
		};
		params.outcome = filters.outcome.map((outcome) => outcomeMap[outcome] || outcome).join(",");
	}

	if (filters.actorType?.length) {
		params["agent-type"] = filters.actorType.join(",");
	}

	if (filters.actorId) {
		params.agent = filters.actorId;
	}

	if (filters.resourceType) {
		params["entity-type"] = filters.resourceType;
	}

	if (filters.resourceId) {
		params.entity = filters.resourceId;
	}

	if (filters.startTime) {
		params.date = `ge${filters.startTime}`;
	}

	if (filters.endTime) {
		if (params.date) {
			params["date:le"] = filters.endTime;
		} else {
			params.date = `le${filters.endTime}`;
		}
	}

	if (filters.ipAddress) {
		params.address = filters.ipAddress;
	}

	params._count = 50;
	params._sort = "-date";

	return params;
}

export function transformFhirAuditEvent(resource: FhirResource): AuditEvent {
	const subtypeCode = resource.subtype?.[0]?.code || "unknown";
	const action = subtypeCode as AuditAction;
	const outcomeCode = resource.outcome as string;
	const agent = resource.agent?.[0];
	const agentTypeCode = agent?.type?.coding?.[0]?.code;
	const entity = resource.entity?.[0];

	return {
		resourceType: "AuditEvent",
		id: resource.id || "",
		timestamp: resource.recorded || new Date().toISOString(),
		action,
		actionCode: resource.action,
		outcome: getAuditOutcome(outcomeCode),
		outcomeCode: outcomeCode as AuditEvent["outcomeCode"],
		outcomeDescription: resource.outcomeDesc,
		actor: {
			type: getAuditActorType(agentTypeCode),
			id: agent?.who?.identifier?.value || agent?.who?.reference,
			name: agent?.who?.display,
			reference: agent?.who?.reference,
		},
		source: {
			observer: resource.source?.observer?.display,
			site: resource.source?.site,
			ipAddress: getAuditExtensionValue(resource, "audit-source-ip"),
			userAgent: getAuditExtensionValue(resource, "audit-user-agent"),
		},
		target: entity
			? {
					reference: entity.what?.reference,
					resourceType: entity.what?.type || entity.what?.reference?.split("/")[0],
					resourceId: entity.what?.reference?.split("/")[1],
					query: entity.description,
				}
			: undefined,
		context: {
			requestId: getAuditExtensionValue(resource, "audit-request-id"),
			sessionId: getAuditExtensionValue(resource, "audit-session-id"),
			clientId: getAuditExtensionValue(resource, "audit-client-id"),
		},
	};
}

export function transformAuditBundleToList(bundle: FhirBundle): AuditEventListResponse {
	const events = (bundle.entry || []).map((entry) => transformFhirAuditEvent(entry.resource));
	const nextLink = bundle.link?.find((link) => link.relation === "next");

	return {
		events,
		total: bundle.total || events.length,
		hasMore: Boolean(nextLink),
		nextCursor: nextLink?.url,
	};
}

function getAuditOutcome(outcomeCode: string): AuditOutcome {
	if (outcomeCode === "4") return "partial";
	if (outcomeCode === "8" || outcomeCode === "12") return "failure";
	return "success";
}

function getAuditActorType(agentTypeCode?: string): AuditEvent["actor"]["type"] {
	if (agentTypeCode === "110153") return "user";
	if (agentTypeCode === "110150") return "client";
	return "system";
}

function getAuditExtensionValue(resource: FhirResource, code: string): string | undefined {
	const url = `http://octofhir.io/StructureDefinition/${code}`;
	return resource.extension?.find((extension: { url: string }) => extension.url === url)?.valueString;
}

