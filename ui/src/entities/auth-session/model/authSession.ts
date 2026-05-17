import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import { isRecord } from "@/shared/api/guards";
import type { FhirResource } from "@/shared/api/types";

dayjs.extend(relativeTime);

export interface AuthSession extends FhirResource {
	resourceType: "AuthSession";
	status: "active" | "expired" | "revoked";
	sessionToken: string;
	subject: {
		reference: string;
	};
	deviceName?: string;
	userAgent?: string;
	ipAddress?: string;
	createdAt: string;
	lastActivityAt?: string;
	expiresAt: string;
}

export type SessionDeviceKind = "desktop" | "mobile";

export interface AuthSessionDeviceView {
	kind: SessionDeviceKind;
	browser: string;
	deviceName: string;
}

export interface AuthSessionActivityView {
	lastActive: string;
	expires: string;
	location: string;
}

export function isAuthSession(resource: unknown): resource is AuthSession {
	return (
		isRecord(resource) &&
		resource.resourceType === "AuthSession" &&
		typeof resource.sessionToken === "string" &&
		isRecord(resource.subject) &&
		typeof resource.subject.reference === "string" &&
		typeof resource.createdAt === "string" &&
		typeof resource.expiresAt === "string" &&
		(resource.status === "active" ||
			resource.status === "expired" ||
			resource.status === "revoked")
	);
}

export function parseAuthSession(resource: unknown): AuthSession | null {
	return isAuthSession(resource) ? resource : null;
}

export function extractAuthSessionUserId(session: AuthSession): string {
	const match = session.subject.reference.match(/User\/(.+)/);
	return match ? match[1] : "";
}

export function isCurrentAuthSession(session: AuthSession, cookieToken?: string): boolean {
	return cookieToken ? session.sessionToken === cookieToken : false;
}

export function getAuthSessionDeviceView(session: AuthSession): AuthSessionDeviceView {
	const userAgent = session.userAgent?.toLowerCase() ?? "";
	const isMobile =
		userAgent.includes("mobile") ||
		userAgent.includes("android") ||
		userAgent.includes("iphone") ||
		userAgent.includes("tablet") ||
		userAgent.includes("ipad");

	return {
		kind: isMobile ? "mobile" : "desktop",
		browser: getBrowserName(session.userAgent),
		deviceName: session.deviceName || (isMobile ? "Mobile Device" : "Desktop Device"),
	};
}

export function getAuthSessionActivityView(session: AuthSession): AuthSessionActivityView {
	return {
		lastActive: session.lastActivityAt ? dayjs(session.lastActivityAt).fromNow() : "Just now",
		expires: dayjs(session.expiresAt).fromNow(),
		location: session.ipAddress || "Unknown",
	};
}

function getBrowserName(userAgent = ""): string {
	const ua = userAgent.toLowerCase();
	if (ua.includes("edg/") || ua.includes("edge")) return "Edge";
	if (ua.includes("firefox")) return "Firefox";
	if (ua.includes("chrome") || ua.includes("chromium")) return "Chrome";
	if (ua.includes("safari")) return "Safari";
	return "Unknown Browser";
}
