import { ReferenceLink, ResourceTypeBadge, type FhirResourceCategory } from "#/shared/fhir";
import { SectionPanel, StatusBadge, type StatusTone } from "#/shared/ui";
import classes from "./FhirResourceWidgets.module.css";

export interface ResourceActivityItem {
    id: string;
    resourceType: string;
    resourceId?: string;
    category?: FhirResourceCategory;
    action: string;
    actor?: string;
    occurredAt?: string;
    tone?: StatusTone;
    href?: string;
}

export interface ResourceActivityPanelProps {
    title?: string;
    items: ResourceActivityItem[];
    emptyText?: string;
}

const activityToneClassName: Record<StatusTone, string> = {
    neutral: classes.activityDotNeutral,
    info: classes.activityDotInfo,
    success: classes.activityDotSuccess,
    warning: classes.activityDotWarning,
    danger: classes.activityDotDanger,
};

export function ResourceActivityPanel({
    title = "Resource activity",
    items,
    emptyText = "No activity",
}: ResourceActivityPanelProps) {
    return (
        <SectionPanel title={title} view="outlined" padding="m">
            {items.length ? (
                <div className={classes.activity}>
                    {items.map((item) => {
                        const reference = item.resourceId
                            ? `${item.resourceType}/${item.resourceId}`
                            : item.resourceType;

                        return (
                            <div key={item.id} className={classes.activityItem}>
                                <span
                                    className={[
                                        classes.activityDot,
                                        activityToneClassName[item.tone ?? "neutral"],
                                    ].join(" ")}
                                />
                                <div className={classes.activityBody}>
                                    <div className={classes.activityTop}>
                                        <ResourceTypeBadge
                                            resourceType={item.resourceType}
                                            category={item.category}
                                        />
                                        <StatusBadge tone={item.tone ?? "neutral"}>{item.action}</StatusBadge>
                                    </div>
                                    <div className={classes.activityText}>
                                        {item.href ? (
                                            <ReferenceLink href={item.href} reference={reference} />
                                        ) : (
                                            reference
                                        )}
                                    </div>
                                    <div className={classes.activityMeta}>
                                        {[item.actor, formatDateTime(item.occurredAt)]
                                            .filter(Boolean)
                                            .join(" · ")}
                                    </div>
                                </div>
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

function formatDateTime(value: string | undefined) {
    if (!value) return undefined;
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return value;
    return date.toLocaleString(undefined, {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
    });
}
