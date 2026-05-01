import type { UserResource, UserSession } from "@/shared/api/types";

export interface PasswordStrengthView {
	score: number;
	label: "weak" | "fair" | "good" | "strong";
}

export interface UserStatusView {
	label: "Active" | "Inactive" | "Locked";
	color: string;
	theme: "success" | "danger" | "unknown";
}

export interface UserRoleView {
	role: string;
	theme: "danger" | "info";
}

export function getPasswordStrength(password: string): PasswordStrengthView {
	let score = 0;
	if (password.length >= 8) score++;
	if (password.length >= 12) score++;
	if (/[a-z]/.test(password) && /[A-Z]/.test(password)) score++;
	if (/\d/.test(password)) score++;
	if (/[^a-zA-Z0-9]/.test(password)) score++;

	const labels: PasswordStrengthView["label"][] = ["weak", "weak", "fair", "good", "strong"];
	return { score: Math.min(score, 4), label: labels[score] || "weak" };
}

export function getUserInitials(user: Pick<UserResource, "name" | "username">): string {
	if (user.name) {
		return user.name
			.split(" ")
			.map((part) => part[0])
			.join("")
			.toUpperCase()
			.slice(0, 2);
	}
	return user.username.slice(0, 2).toUpperCase();
}

export function getUserStatusView(user: Pick<UserResource, "active" | "status">): UserStatusView {
	if (user.status === "locked") {
		return { label: "Locked", color: "red", theme: "danger" };
	}
	if (user.active) {
		return { label: "Active", color: "green", theme: "success" };
	}
	return { label: "Inactive", color: "gray", theme: "unknown" };
}

export function getUserRoleView(role: string): UserRoleView {
	return {
		role,
		theme: role === "admin" ? "danger" : "info",
	};
}

export function formatUserLastLogin(date: string | undefined): string {
	if (!date) return "Never";
	const loginDate = new Date(date);
	const days = Math.floor((Date.now() - loginDate.getTime()) / (1000 * 60 * 60 * 24));

	if (days === 0) return "Today";
	if (days === 1) return "Yesterday";
	if (days < 7) return `${days} days ago`;
	if (days < 30) return `${Math.floor(days / 7)} weeks ago`;
	return loginDate.toLocaleDateString();
}

export function formatUserDateTime(date: string | undefined): string {
	if (!date) return "Never";
	return new Date(date).toLocaleDateString(undefined, {
		year: "numeric",
		month: "short",
		day: "numeric",
		hour: "2-digit",
		minute: "2-digit",
	});
}

export function formatUserRelativeTime(date: string | undefined): string {
	if (!date) return "Never";
	const value = new Date(date);
	const diff = Date.now() - value.getTime();
	const minutes = Math.floor(diff / (1000 * 60));
	const hours = Math.floor(diff / (1000 * 60 * 60));
	const days = Math.floor(diff / (1000 * 60 * 60 * 24));

	if (minutes < 1) return "Just now";
	if (minutes < 60) return `${minutes}m ago`;
	if (hours < 24) return `${hours}h ago`;
	if (days < 7) return `${days}d ago`;
	return value.toLocaleDateString();
}

export function getUserSessionBrowser(session: Pick<UserSession, "userAgent">): string {
	const userAgent = session.userAgent ?? "";
	if (userAgent.includes("Edge")) return "Edge Browser";
	if (userAgent.includes("Firefox")) return "Firefox Browser";
	if (userAgent.includes("Chrome")) return "Chrome Browser";
	if (userAgent.includes("Safari")) return "Safari Browser";
	return "Unknown Browser";
}

