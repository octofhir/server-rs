import type { ReactNode } from "react";
import { ResourceTypeBadge } from "#/shared/fhir";
import { SectionPanel, StatusBadge } from "#/shared/ui";
import classes from "./FhirPackageWidgets.module.css";
import type { FhirPackageResourceTypeSummary } from "./types";

export interface FhirPackageResourceTypesPanelProps {
    title?: ReactNode;
    description?: ReactNode;
    resourceTypes: FhirPackageResourceTypeSummary[];
    selectedResourceType?: string;
    emptyText?: string;
    onSelectResourceType?: (resourceType: FhirPackageResourceTypeSummary) => void;
}

export function FhirPackageResourceTypesPanel({
    title = "Resource types",
    description,
    resourceTypes,
    selectedResourceType,
    emptyText = "No resource types in this package",
    onSelectResourceType,
}: FhirPackageResourceTypesPanelProps) {
    return (
        <SectionPanel
            title={title}
            description={description}
            actions={<StatusBadge tone="info">{resourceTypes.length.toLocaleString()} types</StatusBadge>}
            view="outlined"
            padding="m"
        >
            {resourceTypes.length ? (
                <div className={classes.resourceTypeList}>
                    {resourceTypes.map((item) => {
                        const Element = onSelectResourceType ? "button" : "div";

                        return (
                            <Element
                                key={item.resourceType}
                                type={onSelectResourceType ? "button" : undefined}
                                className={[
                                    classes.resourceTypeItem,
                                    onSelectResourceType ? classes.resourceTypeItemButton : undefined,
                                    item.resourceType === selectedResourceType
                                        ? classes.resourceTypeItemSelected
                                        : undefined,
                                ]
                                    .filter(Boolean)
                                    .join(" ")}
                                onClick={
                                    onSelectResourceType ? () => onSelectResourceType(item) : undefined
                                }
                            >
                                <div className={classes.resourceTypeMain}>
                                    <ResourceTypeBadge resourceType={item.resourceType} />
                                    <StatusBadge tone={item.tone ?? "neutral"}>{item.resourceType}</StatusBadge>
                                </div>
                                <div>
                                    <div className={classes.count}>{item.count.toLocaleString()}</div>
                                    <div className={classes.caption}>definitions</div>
                                </div>
                            </Element>
                        );
                    })}
                </div>
            ) : (
                <div className={classes.empty}>{emptyText}</div>
            )}
        </SectionPanel>
    );
}
