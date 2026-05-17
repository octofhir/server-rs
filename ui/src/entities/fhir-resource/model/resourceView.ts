import type { FhirBundle, FhirResource } from "@/shared/api/types";
import { getBundleResources } from "@/shared/api/guards";

export interface FhirResourceListView {
	id: string;
	resourceId?: string;
	resourceType: string;
	statusLabel: string;
	lastUpdatedLabel: string;
	versionLabel: string;
	canOpen: boolean;
}

export function getFhirBundleResources(bundle: FhirBundle | null | undefined): FhirResource[] {
	return getBundleResources(bundle);
}

export function getFhirResourceDisplayValue(
	resource: FhirResource,
	field: string,
): string {
	const value = resource[field];
	if (value === undefined || value === null) return "-";
	if (typeof value === "string") return value;
	if (typeof value === "boolean") return value ? "true" : "false";
	if (typeof value === "number") return String(value);
	return JSON.stringify(value);
}

export function getFhirResourceListViews(
	resources: FhirResource[],
): FhirResourceListView[] {
	return resources.map((resource, index) => ({
		id: resource.id ?? `${resource.resourceType}-${index}`,
		resourceId: resource.id,
		resourceType: resource.resourceType,
		statusLabel: getFhirResourceDisplayValue(resource, "status"),
		lastUpdatedLabel: resource.meta?.lastUpdated
			? new Date(resource.meta.lastUpdated).toLocaleString()
			: "-",
		versionLabel: `v${resource.meta?.versionId ?? "1"}`,
		canOpen: Boolean(resource.id),
	}));
}
