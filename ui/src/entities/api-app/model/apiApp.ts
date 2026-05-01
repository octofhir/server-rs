import type { FhirResource } from "@/shared/api/types";

export type AppStatus = "active" | "inactive" | "suspended";

export interface OperationPolicy {
	roles?: string[];
	scopes?: string[];
	requireAuth?: boolean;
	requireFhirUser?: boolean;
	compartment?: string;
	script?: string;
}

export type PathSegment = string | { name: string };

export interface InlineOperation {
	id: string;
	method: string;
	path: PathSegment[];
	public?: boolean;
	policy?: OperationPolicy;
}

export interface SubscriptionTrigger {
	resourceType: string;
	event: "create" | "update" | "delete";
	fhirpath?: string;
}

export interface SubscriptionChannel {
	type: string;
	endpoint: string;
}

export interface NotificationDef {
	provider?: string;
	channel: string | string[];
	template: string;
	recipient?: { fhirpath: string };
	delay?: { relativeTo: string; offset: string };
}

export interface InlineSubscription {
	id: string;
	trigger: SubscriptionTrigger;
	channel?: SubscriptionChannel;
	notification?: NotificationDef;
}

export interface AppEndpoint {
	url: string;
	timeout?: number;
}

export interface AppResource extends FhirResource {
	resourceType: "App";
	name: string;
	description?: string;
	apiVersion?: number;
	status?: AppStatus;
	secret?: string;
	endpoint?: AppEndpoint;
	operations?: InlineOperation[];
	subscriptions?: InlineSubscription[];
	resources?: string;
	basePath?: string;
	active?: boolean;
	authentication?: {
		type: string;
		required: boolean;
	};
}

export interface AppStatusView {
	status: AppStatus;
	color: string;
}

export interface AppAccessView {
	label: string;
	color: string;
	description: string;
}

export interface AppMethodView {
	method: string;
	color: string;
}

export interface SubscriptionEventView {
	event: string;
	color: string;
}

const statusColorById: Record<AppStatus, string> = {
	active: "green",
	inactive: "gray",
	suspended: "fire",
};

const methodColorById: Record<string, string> = {
	GET: "primary",
	POST: "warm",
	PUT: "deep",
	DELETE: "fire",
	PATCH: "warm",
};

const subscriptionEventColorById: Record<string, string> = {
	create: "green",
	update: "blue",
	delete: "fire",
};

export function getAppStatus(app: AppResource): AppStatus {
	return app.status ?? (app.active ? "active" : "inactive");
}

export function getAppStatusView(app: AppResource): AppStatusView {
	const status = getAppStatus(app);

	return {
		status,
		color: statusColorById[status] ?? "gray",
	};
}

export function getAppMethodView(method: string): AppMethodView {
	return {
		method,
		color: methodColorById[method] ?? "gray",
	};
}

export function getAppOperationAccessView(isPublic?: boolean): AppAccessView {
	return isPublic
		? {
				label: "Public",
				color: "primary",
				description: "No authentication required",
			}
		: {
				label: "Protected",
				color: "deep",
				description: "Authentication required",
			};
}

export function getSubscriptionEventView(event: string): SubscriptionEventView {
	return {
		event,
		color: subscriptionEventColorById[event] ?? "gray",
	};
}

export function formatAppOperationPath(path: PathSegment[] | string | undefined): string {
	if (!path) return "/";
	if (typeof path === "string") return path.startsWith("/") ? path : `/${path}`;

	const formatted = path
		.map((segment) => (typeof segment === "string" ? segment : `:${segment.name}`))
		.join("/");

	return `/${formatted}`;
}

export function getAppEndpointDisplay(app: AppResource): string {
	return app.endpoint?.url ?? app.basePath ?? "Not configured";
}

