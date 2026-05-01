import type { FhirResource } from "@/shared/api/types";

export interface ClientResource extends FhirResource {
	resourceType: "Client";
	clientId: string;
	clientSecret?: string;
	name: string;
	description?: string;
	grantTypes: string[];
	redirectUris: string[];
	postLogoutRedirectUris: string[];
	scopes: string[];
	confidential: boolean;
	active: boolean;
	accessTokenLifetime?: number;
	refreshTokenLifetime?: number;
	pkceRequired?: boolean;
	allowedOrigins: string[];
	jwksUri?: string;
}

export interface RegenerateSecretResponse {
	clientId: string;
	clientSecret: string;
}

export interface ClientTypeView {
	label: string;
	color: string;
}

export interface ClientStatusView {
	label: string;
	color: string;
}

export const oauthGrantTypeOptions = [
	{ label: "Authorization Code", value: "authorization_code" },
	{ label: "Client Credentials", value: "client_credentials" },
	{ label: "Refresh Token", value: "refresh_token" },
	{ label: "Password", value: "password" },
];

export function getClientTypeView(client: ClientResource): ClientTypeView {
	return client.confidential
		? { label: "Confidential", color: "blue" }
		: { label: "Public", color: "gray" };
}

export function getClientStatusView(client: ClientResource): ClientStatusView {
	return client.active
		? { label: "Active", color: "green" }
		: { label: "Inactive", color: "gray" };
}

