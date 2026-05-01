import { CapabilityFlagsPanel, type CapabilityFlagItem } from "./CapabilityFlagsPanel";
import { ChecklistPanel, type ChecklistItem } from "./ChecklistPanel";
import { FhirSurfacePanel, type FhirSurfacePanelProps } from "./FhirSurfacePanel";
import classes from "./FhirDashboardPanels.module.css";

export interface FhirDashboardAsideProps {
    surface: FhirSurfacePanelProps;
    capabilities: CapabilityFlagItem[];
    checklist?: {
        title: string;
        items: ChecklistItem[];
    };
}

export function FhirDashboardAside({ surface, capabilities, checklist }: FhirDashboardAsideProps) {
    return (
        <div className={classes.stack}>
            <FhirSurfacePanel {...surface} />
            <CapabilityFlagsPanel capabilities={capabilities} />
            {checklist ? <ChecklistPanel title={checklist.title} items={checklist.items} /> : null}
        </div>
    );
}
