import type { AuditAction, AuditEvent, AuditOutcome } from "@/shared/api/types";

export interface AuditTimestampView {
	time: string;
	relative: string;
	full: string;
}

export interface AuditTargetView {
	primary: string;
	secondary?: string;
}

const actionLabels: Record<AuditAction, string> = {
	"user.login": "User Login",
	"user.logout": "User Logout",
	"user.login_failed": "Login Failed",
	"resource.create": "Create",
	"resource.read": "Read",
	"resource.update": "Update",
	"resource.delete": "Delete",
	"resource.search": "Search",
	"policy.evaluate": "Policy Check",
	"client.auth": "Client Auth",
	"client.create": "Client Created",
	"client.update": "Client Updated",
	"client.delete": "Client Deleted",
	"config.change": "Config Change",
	"system.startup": "System Start",
	"system.shutdown": "System Stop",
};

const actionDetailLabels: Record<AuditAction, string> = {
	"user.login": "User Login",
	"user.logout": "User Logout",
	"user.login_failed": "Login Failed",
	"resource.create": "Resource Created",
	"resource.read": "Resource Read",
	"resource.update": "Resource Updated",
	"resource.delete": "Resource Deleted",
	"resource.search": "Resource Search",
	"policy.evaluate": "Policy Evaluated",
	"client.auth": "Client Authentication",
	"client.create": "Client Created",
	"client.update": "Client Updated",
	"client.delete": "Client Deleted",
	"config.change": "Configuration Changed",
	"system.startup": "System Started",
	"system.shutdown": "System Stopped",
};

export function getAuditActionLabel(action: AuditAction): string {
	return actionLabels[action] || action;
}

export function getAuditActionDetailLabel(action: AuditAction): string {
	return actionDetailLabels[action] || getAuditActionLabel(action);
}

export function getAuditActionColor(action: AuditAction): string {
	if (action.startsWith("user.login_failed")) return "red";
	if (action.includes("delete")) return "red";
	if (action.includes("create")) return "green";
	if (action.includes("update") || action.includes("change")) return "yellow";
	if (action.includes("login")) return "teal";
	if (action.includes("logout")) return "gray";
	return "blue";
}

export function getAuditOutcomeColor(outcome: AuditOutcome): string {
	switch (outcome) {
		case "success":
			return "green";
		case "failure":
			return "red";
		case "partial":
			return "yellow";
	}
}

export function getAuditOutcomeLabel(outcome: AuditOutcome): string {
	return outcome.charAt(0).toUpperCase() + outcome.slice(1);
}

export function getAuditTimestampView(timestamp: string): AuditTimestampView {
	const then = new Date(timestamp);

	return {
		time: then.toLocaleTimeString(),
		relative: formatRelativeTime(then),
		full: then.toLocaleString(),
	};
}

export function getAuditActorLabel(event: AuditEvent): string {
	return event.actor.name || event.actor.id || event.actor.type;
}

export function getAuditTargetView(event: AuditEvent): AuditTargetView | null {
	if (!event.target) return null;

	return {
		primary: `${event.target.resourceType}${event.target.resourceId ? `/${event.target.resourceId}` : ""}`,
		secondary: event.target.query,
	};
}

function formatRelativeTime(then: Date): string {
	const diff = Date.now() - then.getTime();
	const seconds = Math.floor(diff / 1000);
	const minutes = Math.floor(seconds / 60);
	const hours = Math.floor(minutes / 60);
	const days = Math.floor(hours / 24);

	if (seconds < 60) return "Just now";
	if (minutes < 60) return `${minutes}m ago`;
	if (hours < 24) return `${hours}h ago`;
	if (days < 7) return `${days}d ago`;
	return then.toLocaleDateString();
}
