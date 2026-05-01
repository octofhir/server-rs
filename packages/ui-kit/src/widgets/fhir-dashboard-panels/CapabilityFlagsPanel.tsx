import { CapabilityFlag } from "#/shared/fhir";
import { SectionPanel } from "#/shared/ui";
import classes from "./FhirDashboardPanels.module.css";

export interface CapabilityFlagItem {
    id: string;
    label: string;
    enabled?: boolean;
}

export interface CapabilityFlagsPanelProps {
    title?: string;
    capabilities: CapabilityFlagItem[];
    emptyText?: string;
}

export function CapabilityFlagsPanel({
    title = "Capabilities",
    capabilities,
    emptyText = "No capabilities reported",
}: CapabilityFlagsPanelProps) {
    return (
        <SectionPanel title={title} view="outlined" padding="m">
            {capabilities.length ? (
                <div className={classes.flags}>
                    {capabilities.map((capability) => (
                        <CapabilityFlag
                            key={capability.id}
                            label={capability.label}
                            enabled={capability.enabled}
                        />
                    ))}
                </div>
            ) : (
                <div className={classes.empty}>{emptyText}</div>
            )}
        </SectionPanel>
    );
}
