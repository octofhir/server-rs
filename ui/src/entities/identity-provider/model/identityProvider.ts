import type { FhirResource } from "@/shared/api/types";

export type IdentityProviderType = "oidc" | "oauth2" | "saml";

export interface ClaimToRoles {
	claim: string;
	value: string;
	roles: string[];
}

export interface IdentityProviderResource extends FhirResource {
	resourceType: "IdentityProvider";
	name: string;
	displayName?: string;
	logoUrl?: string;
	type: IdentityProviderType;
	issuer: string;
	clientId?: string;
	clientSecret?: string;
	samlMetadata?: string;
	authorizationEndpoint?: string;
	tokenEndpoint?: string;
	userinfoEndpoint?: string;
	scopes?: string[];
	active: boolean;
	userMapping?: {
		subjectClaim?: string;
		usernameClaim?: string;
		emailClaim?: string;
		rolesClaim?: string;
		claimsToRoles?: ClaimToRoles[];
	};
}

export interface IdentityProviderTypeView {
	label: string;
	color: string;
}

export interface IdentityProviderStatusView {
	label: string;
	color: string;
}

export const identityProviderTypeOptions: Array<{
	label: string;
	value: IdentityProviderType;
}> = [
	{ label: "OpenID Connect", value: "oidc" },
	{ label: "OAuth 2.0", value: "oauth2" },
	{ label: "SAML 2.0", value: "saml" },
];

const typeViewById: Record<IdentityProviderType, IdentityProviderTypeView> = {
	oidc: { label: "OIDC", color: "blue" },
	oauth2: { label: "OAuth 2.0", color: "gray" },
	saml: { label: "SAML", color: "gray" },
};

export function getIdentityProviderTypeView(
	type: IdentityProviderType | undefined,
): IdentityProviderTypeView {
	return type ? typeViewById[type] : { label: "Unknown", color: "gray" };
}

export function getIdentityProviderStatusView(
	provider: IdentityProviderResource,
): IdentityProviderStatusView {
	return provider.active
		? { label: "Active", color: "green" }
		: { label: "Inactive", color: "gray" };
}

