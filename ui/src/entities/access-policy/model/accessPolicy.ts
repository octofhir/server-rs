import type { FhirResource } from "@/shared/api/types";

export type AccessPolicyEngineType = "allow" | "deny" | "quickjs";

/**
 * AccessPolicy resource matching backend structure.
 *
 * Uses matcher + engine pattern for policy evaluation.
 */
export interface AccessPolicyResource extends FhirResource {
	resourceType: "AccessPolicy";
	name: string;
	description?: string;
	active?: boolean;
	priority?: number;
	matcher?: MatcherElement;
	engine: EngineElement;
	denyMessage?: string;
}

/**
 * Matcher element - determines when a policy applies.
 * All specified fields must match for the policy to apply (AND logic).
 */
export interface MatcherElement {
	/** Client ID patterns (supports wildcards with `*`). */
	clients?: string[];
	/** Required user roles (any role matches). */
	roles?: string[];
	/** User FHIR resource types (e.g., "Practitioner", "Patient"). */
	userTypes?: string[];
	/** Target FHIR resource types. */
	resourceTypes?: string[];
	/** FHIR operations (e.g., "read", "create", "search"). */
	operations?: string[];
	/** Operation IDs for more specific targeting (e.g., "fhir.read", "graphql.query"). */
	operationIds?: string[];
	/** Request path patterns (glob syntax). */
	paths?: string[];
	/** Source IP addresses in CIDR notation. */
	sourceIps?: string[];
}

/**
 * Engine element - how the policy is evaluated.
 */
export interface EngineElement {
	/** Engine type: allow, deny, or quickjs for custom scripts. */
	type: AccessPolicyEngineType;
	/** Script content (required for QuickJS engine). */
	script?: string;
}

export interface AccessPolicyEngineView {
	label: string;
	color: string;
}

export interface AccessPolicyStatusView {
	label: string;
	color: string;
}

export const accessPolicyOperations = [
	"read",
	"vread",
	"update",
	"patch",
	"delete",
	"history",
	"history-instance",
	"history-type",
	"history-system",
	"create",
	"search",
	"search-type",
	"search-system",
	"capabilities",
	"batch",
	"transaction",
	"operation",
	"*",
] as const;

export const accessPolicyUserTypes = [
	"Practitioner",
	"Patient",
	"RelatedPerson",
	"Person",
	"*",
] as const;

const engineViewByType: Record<AccessPolicyEngineType, AccessPolicyEngineView> = {
	allow: { label: "Allow", color: "green" },
	deny: { label: "Deny", color: "red" },
	quickjs: { label: "QuickJS Script", color: "blue" },
};

export function getAccessPolicyEngineView(
	type: AccessPolicyEngineType | undefined,
): AccessPolicyEngineView {
	return type ? engineViewByType[type] : { label: "Unknown", color: "gray" };
}

export function getAccessPolicyStatusView(
	policy: Pick<AccessPolicyResource, "active">,
): AccessPolicyStatusView {
	return policy.active !== false
		? { label: "Active", color: "green" }
		: { label: "Inactive", color: "gray" };
}

export function getAccessPolicyPriority(policy: Pick<AccessPolicyResource, "priority">) {
	return policy.priority ?? 100;
}

