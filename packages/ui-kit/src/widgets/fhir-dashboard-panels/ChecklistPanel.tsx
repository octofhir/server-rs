import { SectionPanel } from "#/shared/ui";
import classes from "./FhirDashboardPanels.module.css";

export interface ChecklistItem {
    id: string;
    label: string;
    description?: string;
}

export interface ChecklistPanelProps {
    title: string;
    items: ChecklistItem[];
    emptyText?: string;
}

export function ChecklistPanel({ title, items, emptyText = "No checklist items" }: ChecklistPanelProps) {
    return (
        <SectionPanel title={title} view="outlined" padding="m">
            {items.length ? (
                <div className={classes.checklist}>
                    {items.map((item) => (
                        <div key={item.id} className={classes.checklistItem}>
                            <span className={classes.marker} />
                            <span className={classes.checklistText}>
                                <span>{item.label}</span>
                                {item.description ? (
                                    <span className={classes.checklistDescription}>
                                        {item.description}
                                    </span>
                                ) : null}
                            </span>
                        </div>
                    ))}
                </div>
            ) : (
                <div className={classes.empty}>{emptyText}</div>
            )}
        </SectionPanel>
    );
}
