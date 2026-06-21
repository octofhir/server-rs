import type {
	AuditAction,
	AuditActionCode,
	AuditEvent,
	AuditEventListResponse,
	AuditEventUIFilters,
	AuditOutcome,
	AuditOutcomeCode,
	FhirBundle,
	FhirResource,
} from "@/shared/api/types";

const AUDIT_ACTIONS = [
	"user.login",
	"user.logout",
	"user.login_failed",
	"resource.create",
	"resource.read",
	"resource.update",
	"resource.delete",
	"resource.search",
	"policy.evaluate",
	"client.auth",
	"client.create",
	"client.update",
	"client.delete",
	"config.change",
	"system.startup",
	"system.shutdown",
] satisfies AuditAction[];

function isAuditAction(value: string | undefined): value is AuditAction {
	return value !== undefined && (AUDIT_ACTIONS as readonly string[]).includes(value);
}

function isAuditActionCode(value: unknown): value is AuditActionCode {
	return value === "C" || value === "R" || value === "U" || value === "D" || value === "E";
}

function isAuditOutcomeCode(value: unknown): value is AuditOutcomeCode {
	return value === "0" || value === "4" || value === "8" || value === "12";
}

function fallbackAuditAction(actionCode: unknown): AuditAction {
	switch (actionCode) {
		case "C":
			return "resource.create";
		case "U":
			return "resource.update";
		case "D":
			return "resource.delete";
		case "E":
			return "policy.evaluate";
		case "R":
		default:
			return "resource.read";
	}
}

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
	const r = resource as FhirResource & {
		subtype?: { code?: string }[];
		action?: string;
		outcome?: string;
		recorded?: string;
		outcomeDesc?: string;
		agent?: Array<{
			type?: { coding?: { code?: string }[] };
			who?: { identifier?: { value?: string }; reference?: string; display?: string };
		}>;
		source?: { observer?: { display?: string }; site?: string };
		entity?: Array<{ what?: { reference?: string; type?: string }; description?: string }>;
	};
	const subtypeCode = r.subtype?.[0]?.code;
	const action = isAuditAction(subtypeCode) ? subtypeCode : fallbackAuditAction(r.action);
	const actionCode = isAuditActionCode(r.action) ? r.action : undefined;
	const outcomeCode = isAuditOutcomeCode(r.outcome) ? r.outcome : undefined;
	const agent = r.agent?.[0];
	const agentTypeCode = agent?.type?.coding?.[0]?.code;
	const entity = r.entity?.[0];

	return {
		resourceType: "AuditEvent",
		id: resource.id || "",
		timestamp: r.recorded || new Date().toISOString(),
		action,
		actionCode,
		outcome: getAuditOutcome(outcomeCode),
		outcomeCode,
		outcomeDescription: r.outcomeDesc,
		actor: {
			type: getAuditActorType(agentTypeCode),
			id: agent?.who?.identifier?.value || agent?.who?.reference,
			name: agent?.who?.display,
			reference: agent?.who?.reference,
		},
		source: {
			observer: r.source?.observer?.display,
			site: r.source?.site,
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
	const events = (bundle.entry || [])
		.map((entry) => entry.resource)
		.filter((r): r is FhirResource => r != null)
		.map(transformFhirAuditEvent);
	const nextLink = bundle.link?.find((link) => link.relation === "next");

	return {
		events,
		total: bundle.total || events.length,
		hasMore: Boolean(nextLink),
		nextCursor: nextLink?.url,
	};
}

function getAuditOutcome(outcomeCode: AuditOutcomeCode | undefined): AuditOutcome {
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
	return (resource as { extension?: { url: string; valueString?: string }[] }).extension?.find(
		(extension) => extension.url === url,
	)?.valueString;
}
