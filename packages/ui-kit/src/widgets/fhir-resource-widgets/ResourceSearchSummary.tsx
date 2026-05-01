import { ResourceTypeBadge, type FhirResourceCategory } from "#/shared/fhir";
import { SectionPanel, StatusBadge } from "#/shared/ui";
import classes from "./FhirResourceWidgets.module.css";

export interface ResourceSearchFacet {
    id: string;
    label: string;
    value: string | number;
}

export interface ResourceSearchSummaryProps {
    resourceType: string;
    category?: FhirResourceCategory;
    total?: number;
    queryLabel?: string;
    facets?: ResourceSearchFacet[];
}

export function ResourceSearchSummary({
    resourceType,
    category,
    total,
    queryLabel = "All resources",
    facets = [],
}: ResourceSearchSummaryProps) {
    return (
        <SectionPanel
            title="Search summary"
            actions={<ResourceTypeBadge resourceType={resourceType} category={category} />}
            view="outlined"
            padding="m"
        >
            <div className={classes.summaryGrid}>
                <SummaryCell label="Resource" value={resourceType} />
                <SummaryCell label="Query" value={queryLabel} />
            <SummaryCell label="Total" value={total !== undefined ? total.toLocaleString() : "Unknown"} />
        </div>

            {facets.length ? (
                <div className={classes.summaryMeta}>
                    {facets.map((facet) => (
                        <StatusBadge key={facet.id} tone="neutral">
                            {facet.label}: {facet.value}
                        </StatusBadge>
                    ))}
                </div>
            ) : null}
        </SectionPanel>
    );
}

function SummaryCell({ label, value }: { label: string; value: string }) {
    return (
        <div className={classes.summaryCell}>
            <div className={classes.summaryLabel}>{label}</div>
            <div className={classes.summaryValue}>{value}</div>
        </div>
    );
}
