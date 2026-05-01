import { CanonicalUri, ResourceTypeBadge, type FhirResourceCategory } from "#/shared/fhir";
import { SectionPanel, StatusBadge } from "#/shared/ui";
import classes from "./FhirResourceWidgets.module.css";

export interface ResourceTypeDirectoryItem {
    id: string;
    name: string;
    category?: FhirResourceCategory;
    canonical?: string;
    packageName?: string;
    description?: string;
    count?: number;
}

export interface ResourceTypeDirectoryPanelProps {
    title?: string;
    description?: string;
    resources: ResourceTypeDirectoryItem[];
    selectedResourceType?: string;
    emptyText?: string;
    onSelectResourceType?: (resourceType: string) => void;
}

export function ResourceTypeDirectoryPanel({
    title = "Resource types",
    description = "FHIR, system, and custom resource definitions available on this server.",
    resources,
    selectedResourceType,
    emptyText = "No resource types",
    onSelectResourceType,
}: ResourceTypeDirectoryPanelProps) {
    return (
        <SectionPanel title={title} description={description} view="outlined" padding="m">
            {resources.length ? (
                <div className={classes.directoryList}>
                    {resources.map((resource) => {
                        const isSelected = resource.name === selectedResourceType;
                        const content = (
                            <ResourceTypeDirectoryItemView
                                resource={resource}
                                selected={isSelected}
                            />
                        );

                        return onSelectResourceType ? (
                            <button
                                key={resource.id}
                                className={[
                                    classes.directoryItem,
                                    classes.directoryItemButton,
                                    isSelected ? classes.directoryItemSelected : undefined,
                                ]
                                    .filter(Boolean)
                                    .join(" ")}
                                type="button"
                                onClick={() => onSelectResourceType(resource.name)}
                            >
                                {content}
                            </button>
                        ) : (
                            <div
                                key={resource.id}
                                className={[
                                    classes.directoryItem,
                                    isSelected ? classes.directoryItemSelected : undefined,
                                ]
                                    .filter(Boolean)
                                    .join(" ")}
                            >
                                {content}
                            </div>
                        );
                    })}
                </div>
            ) : (
                <div className={classes.empty}>{emptyText}</div>
            )}
        </SectionPanel>
    );
}

function ResourceTypeDirectoryItemView({
    resource,
    selected,
}: {
    resource: ResourceTypeDirectoryItem;
    selected: boolean;
}) {
    return (
        <>
            <div className={classes.directoryMain}>
                <div className={classes.titleRow}>
                    <span className={classes.name}>{resource.name}</span>
                    <ResourceTypeBadge
                        resourceType={resource.name}
                        category={resource.category}
                    />
                    {selected ? <StatusBadge tone="success">Selected</StatusBadge> : null}
                </div>

                {resource.description ? (
                    <div className={classes.description}>{resource.description}</div>
                ) : null}

                {resource.canonical ? (
                    <div className={classes.directoryCanonical}>
                        <CanonicalUri value={resource.canonical} />
                    </div>
                ) : null}
            </div>

            <div className={classes.directoryMeta}>
                {resource.count !== undefined ? (
                    <span className={classes.directoryCount}>{resource.count.toLocaleString()}</span>
                ) : null}
                {resource.packageName ? (
                    <StatusBadge tone="neutral">{resource.packageName}</StatusBadge>
                ) : null}
            </div>
        </>
    );
}
