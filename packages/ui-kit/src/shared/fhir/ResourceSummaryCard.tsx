import type { ReactNode } from "react";
import { StatusBadge, Surface, type StatusTone } from "../ui";
import { CanonicalUri } from "./CanonicalUri";
import { ResourceMetaBar } from "./ResourceMetaBar";
import { ResourceTypeBadge, type FhirResourceCategory } from "./ResourceTypeBadge";
import classes from "./ResourceSummaryCard.module.css";

export interface ResourceSummaryMeta {
    id: string;
    label: ReactNode;
    tone?: StatusTone;
}

export interface ResourceSummaryCardProps {
    resourceType: string;
    title?: ReactNode;
    description?: ReactNode;
    id?: string;
    canonical?: string;
    category?: FhirResourceCategory;
    versionId?: string;
    lastUpdated?: string;
    profileCount?: number;
    meta?: ResourceSummaryMeta[];
    onClick?: () => void;
}

export function ResourceSummaryCard({
    resourceType,
    title,
    description,
    id,
    canonical,
    category,
    versionId,
    lastUpdated,
    profileCount,
    meta,
    onClick,
}: ResourceSummaryCardProps) {
    return (
        <Surface className={classes.root} interactive={!!onClick} onClick={onClick}>
            <div className={classes.head}>
                <div className={classes.titleBlock}>
                    <div className={classes.title}>{title ?? id ?? resourceType}</div>
                    {description ? <div className={classes.description}>{description}</div> : null}
                </div>
                <ResourceTypeBadge resourceType={resourceType} category={category} />
            </div>

            {canonical ? (
                <div className={classes.canonical}>
                    <CanonicalUri value={canonical} />
                </div>
            ) : null}

            <ResourceMetaBar
                id={id}
                versionId={versionId}
                lastUpdated={lastUpdated}
                profileCount={profileCount}
            />

            {meta?.length ? (
                <div className={classes.meta}>
                    {meta.map((item) => (
                        <StatusBadge key={item.id} tone={item.tone ?? "neutral"}>
                            {item.label}
                        </StatusBadge>
                    ))}
                </div>
            ) : null}
        </Surface>
    );
}
