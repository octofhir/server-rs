import { StatusBadge, type StatusTone } from "../ui";

export type FhirResourceCategory = "fhir" | "system" | "custom" | "unknown";

export interface ResourceTypeBadgeProps {
    resourceType: string;
    category?: FhirResourceCategory;
}

const toneByCategory: Record<FhirResourceCategory, StatusTone> = {
    fhir: "info",
    system: "warning",
    custom: "success",
    unknown: "neutral",
};

export function ResourceTypeBadge({ resourceType, category = "unknown" }: ResourceTypeBadgeProps) {
    return <StatusBadge tone={toneByCategory[category]}>{resourceType}</StatusBadge>;
}
