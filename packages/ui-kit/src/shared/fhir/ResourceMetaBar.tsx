import { StatusBadge } from "../ui";
import classes from "./ResourceMetaBar.module.css";

export interface ResourceMetaBarProps {
    id?: string;
    versionId?: string;
    lastUpdated?: string;
    profileCount?: number;
}

export function ResourceMetaBar({ id, versionId, lastUpdated, profileCount }: ResourceMetaBarProps) {
    return (
        <div className={classes.root}>
            {id ? <StatusBadge tone="neutral">id: {id}</StatusBadge> : null}
            {versionId ? <StatusBadge tone="neutral">v{versionId}</StatusBadge> : null}
            {lastUpdated ? <StatusBadge tone="info">{formatDateTime(lastUpdated)}</StatusBadge> : null}
            {profileCount ? <StatusBadge tone="info">{profileCount} profiles</StatusBadge> : null}
        </div>
    );
}

function formatDateTime(value: string) {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return value;
    return date.toLocaleString(undefined, {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
    });
}
