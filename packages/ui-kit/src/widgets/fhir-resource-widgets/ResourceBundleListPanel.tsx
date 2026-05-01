import { ResourceSummaryCard, type FhirResourceCategory } from "#/shared/fhir";
import { Button, SectionPanel, StatusBadge } from "#/shared/ui";
import classes from "./FhirResourceWidgets.module.css";

export interface ResourceBundleListItem {
    id: string;
    resourceType: string;
    category?: FhirResourceCategory;
    title?: string;
    description?: string;
    canonical?: string;
    versionId?: string;
    lastUpdated?: string;
    status?: string;
    profileCount?: number;
}

export interface ResourceBundleListPanelProps {
    title?: string;
    resourceType?: string;
    total?: number;
    items: ResourceBundleListItem[];
    selectedResourceId?: string;
    emptyText?: string;
    loading?: boolean;
    hasNextPage?: boolean;
    hasPreviousPage?: boolean;
    onSelectResource?: (resource: ResourceBundleListItem) => void;
    onNextPage?: () => void;
    onPreviousPage?: () => void;
}

export function ResourceBundleListPanel({
    title,
    resourceType,
    total,
    items,
    selectedResourceId,
    emptyText = "No resources",
    loading,
    hasNextPage,
    hasPreviousPage,
    onSelectResource,
    onNextPage,
    onPreviousPage,
}: ResourceBundleListPanelProps) {
    const heading = title ?? (resourceType ? `${resourceType} resources` : "Resources");

    return (
        <SectionPanel
            title={heading}
            actions={
                <div className={classes.bundleActions}>
                    {total !== undefined ? (
                        <StatusBadge tone="info">{total.toLocaleString()} total</StatusBadge>
                    ) : null}
                    {onPreviousPage ? (
                        <Button
                            size="s"
                            view="flat-secondary"
                            disabled={!hasPreviousPage || loading}
                            onClick={onPreviousPage}
                        >
                            Previous
                        </Button>
                    ) : null}
                    {onNextPage ? (
                        <Button
                            size="s"
                            view="flat-secondary"
                            disabled={!hasNextPage || loading}
                            onClick={onNextPage}
                        >
                            Next
                        </Button>
                    ) : null}
                </div>
            }
            view="outlined"
            padding="m"
        >
            {items.length ? (
                <div className={classes.bundleList}>
                    {items.map((item) => (
                        <div
                            key={`${item.resourceType}/${item.id}`}
                            className={[
                                classes.bundleItem,
                                item.id === selectedResourceId ? classes.bundleItemSelected : undefined,
                            ]
                                .filter(Boolean)
                                .join(" ")}
                        >
                            <ResourceSummaryCard
                                resourceType={item.resourceType}
                                title={item.title ?? item.id}
                                description={item.description}
                                id={item.id}
                                canonical={item.canonical}
                                category={item.category}
                                versionId={item.versionId}
                                lastUpdated={item.lastUpdated}
                                profileCount={item.profileCount}
                                meta={
                                    item.status
                                        ? [{ id: "status", label: item.status, tone: "neutral" }]
                                        : undefined
                                }
                                onClick={onSelectResource ? () => onSelectResource(item) : undefined}
                            />
                        </div>
                    ))}
                </div>
            ) : (
                <div className={classes.empty}>{loading ? "Loading resources" : emptyText}</div>
            )}
        </SectionPanel>
    );
}
