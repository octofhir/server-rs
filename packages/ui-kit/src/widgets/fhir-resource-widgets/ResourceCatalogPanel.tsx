import { ResourceTypeBadge, type FhirResourceCategory } from "#/shared/fhir";
import { SectionPanel, StatusBadge } from "#/shared/ui";
import classes from "./FhirResourceWidgets.module.css";

export interface ResourceCatalogItem {
    id: string;
    resourceType: string;
    category?: FhirResourceCategory;
    description?: string;
    packageName?: string;
    count?: number;
    onClick?: () => void;
}

export interface ResourceCatalogPanelProps {
    title?: string;
    description?: string;
    resources: ResourceCatalogItem[];
    emptyText?: string;
}

export function ResourceCatalogPanel({
    title = "Resource catalog",
    description,
    resources,
    emptyText = "No resources",
}: ResourceCatalogPanelProps) {
    return (
        <SectionPanel title={title} description={description} view="outlined" padding="m">
            {resources.length ? (
                <div className={classes.list}>
                    {resources.map((resource) => {
                        const content = <ResourceCatalogItemView resource={resource} />;

                        return resource.onClick ? (
                            <button
                                key={resource.id}
                                className={`${classes.catalogItem} ${classes.catalogItemButton}`}
                                type="button"
                                onClick={resource.onClick}
                            >
                                {content}
                            </button>
                        ) : (
                            <div key={resource.id} className={classes.catalogItem}>
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

function ResourceCatalogItemView({ resource }: { resource: ResourceCatalogItem }) {
    return (
        <>
            <div className={classes.titleBlock}>
                <div className={classes.titleRow}>
                    <span className={classes.name}>{resource.resourceType}</span>
                    <ResourceTypeBadge
                        resourceType={resource.resourceType}
                        category={resource.category}
                    />
                </div>

                {resource.description ? (
                    <div className={classes.description}>{resource.description}</div>
                ) : null}

                {resource.packageName ? (
                    <div className={classes.meta}>
                        <StatusBadge tone="neutral">{resource.packageName}</StatusBadge>
                    </div>
                ) : null}
            </div>

            {resource.count !== undefined ? (
                <div className={classes.count}>{resource.count.toLocaleString()}</div>
            ) : null}
        </>
    );
}
