import { SectionPanel, StatGrid, StatusBadge, type StatusTone } from "#/shared/ui";

export interface FhirSurfacePanelProps {
    fhirCount: number;
    systemCount: number;
    customCount: number;
    healthLabel?: string;
    healthTone?: StatusTone;
}

export function FhirSurfacePanel({
    fhirCount,
    systemCount,
    customCount,
    healthLabel,
    healthTone = "neutral",
}: FhirSurfacePanelProps) {
    return (
        <SectionPanel
            title="FHIR surface"
            actions={
                healthLabel ? <StatusBadge tone={healthTone}>{healthLabel}</StatusBadge> : undefined
            }
            view="outlined"
            padding="m"
        >
            <StatGrid
                items={[
                    {
                        id: "fhir",
                        label: "FHIR resources",
                        value: fhirCount,
                        caption: "Core model",
                        tone: "success",
                    },
                    {
                        id: "system",
                        label: "System resources",
                        value: systemCount,
                        caption: "Control plane",
                        tone: "info",
                    },
                    {
                        id: "custom",
                        label: "Custom resources",
                        value: customCount,
                        caption: "Local",
                        tone: customCount > 0 ? "warning" : "neutral",
                    },
                ]}
            />
        </SectionPanel>
    );
}
