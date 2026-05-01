import type { HealthResponse } from "@/shared/api/types";

export type HealthStatus = HealthResponse["status"] | undefined;

export interface HealthStatusView {
	label: string;
	tone: "success" | "warning" | "danger";
	caption: string;
}

export function getHealthStatusView(status: HealthStatus): HealthStatusView {
	if (status === "ok") {
		return {
			label: "Healthy",
			tone: "success",
			caption: "FHIR API is responding",
		};
	}

	if (status === "degraded") {
		return {
			label: "Degraded",
			tone: "warning",
			caption: "Some services need attention",
		};
	}

	return {
		label: "Down",
		tone: "danger",
		caption: "Health endpoint is unavailable",
	};
}
